use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, ensure, Context};
use axum::extract::State;
use axum::Json;
use deadpool_postgres::GenericClient;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::BitcoinHash;
use fmo_api_types::FederationRating;
use nostr_sdk::{
    Event, Filter, FilterOptions, Kind, RelayOptions, RelayPool, RelayPoolOptions,
    RelaySendOptions, SingleLetterTag, Timestamp,
};
use postgres_from_row::FromRow;
use regex::Regex;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::federation::observer::{federation_id_from_db, FederationObserver};
use crate::util::query;
use crate::AppState;

// TODO: move to common crate
const FEDERATION_ANNOUNCEMENT_EVENT_KIND: Kind = Kind::Custom(38173);
const RECOMMENDATION_EVENT_KIND: Kind = Kind::Custom(38000);

/// How often relays are asked for events published since the last sync
const NOSTR_SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// How often everything is fetched again, for events that reached relays
/// later than their timestamp says
const NOSTR_FULL_SYNC_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// A sync asks for events from this long before the previous one started, for
/// publishers whose clocks are a bit off
const NOSTR_SYNC_OVERLAP: Duration = Duration::from_secs(10 * 60);
const NOSTR_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Both event kinds name their federation in a `d` tag
fn federation_id_tag() -> SingleLetterTag {
    SingleLetterTag::from_char('d').expect("Tag is valid")
}

#[derive(Debug, Clone, FromRow)]
struct NostrRelay {
    relay_url: String,
}

impl FederationObserver {
    /// Syncs federation announcements, and the votes on announced and
    /// observed federations.
    ///
    /// Most minutes nothing new was published, so each sync asks only for
    /// events since the previous one, in one request for all federations.
    /// Every few hours, and after a restart, everything is fetched again per
    /// federation, the way every sync used to.
    pub async fn sync_nostr_events(self) -> anyhow::Result<()> {
        let client = self.nostr_relay_client().await?;
        let mut interval = interval(NOSTR_SYNC_INTERVAL);
        let mut last_full_sync: Option<Instant> = None;
        let mut last_sync_start: Option<Timestamp> = None;

        loop {
            interval.tick().await;
            let sync_start = Timestamp::now();
            let since = last_sync_start
                .filter(|_| {
                    last_full_sync.is_some_and(|at| at.elapsed() < NOSTR_FULL_SYNC_INTERVAL)
                })
                .map(|previous| previous - NOSTR_SYNC_OVERLAP);

            let announcements = fetch_events(
                &client,
                vec![since_filter(
                    Filter::new().kind(FEDERATION_ANNOUNCEMENT_EVENT_KIND),
                    since,
                )],
            )
            .await?;
            self.store_new_events::<ParsedFederationEvent>(announcements)
                .await?;

            let federations = self.voted_federation_ids().await?;
            match since {
                Some(since) => {
                    let votes =
                        fetch_events(&client, vec![votes_filter(federations, Some(since))]).await?;
                    self.store_new_events::<ParsedRecommendationEvent>(votes)
                        .await?;
                }
                // One request per federation, so a relay's cap on results per
                // request applies to each federation separately
                None => {
                    for federation_id in federations {
                        let votes =
                            fetch_events(&client, vec![votes_filter([federation_id], None)])
                                .await?;
                        self.store_new_events::<ParsedRecommendationEvent>(votes)
                            .await?;
                    }
                    last_full_sync = Some(Instant::now());
                }
            }
            last_sync_start = Some(sync_start);
        }
    }

    /// The relay pool shared by the sync task and the publish endpoints,
    /// connected on first use. A pool keeps reconnecting to its relays until
    /// it is shut down, so one per request leaked ten connections each time.
    async fn nostr_relay_client(&self) -> anyhow::Result<RelayPool> {
        self.nostr_pool()
            .get_or_try_init(|| self.connect_nostr_relays())
            .await
            .cloned()
    }

    async fn connect_nostr_relays(&self) -> anyhow::Result<RelayPool> {
        let relays = query::<NostrRelay>(
            &self.connection().await?,
            "SELECT relay_url FROM nostr_relays",
            &[],
        )
        .await?
        .into_iter()
        .map(|relay| relay.relay_url)
        .collect::<Vec<_>>();
        let client = RelayPool::new(RelayPoolOptions::default());
        for relay_url in &relays {
            client.add_relay(relay_url, RelayOptions::default()).await?;
        }
        client.connect(Some(Duration::from_secs(5))).await;

        info!(?relays, "Started Nostr client");

        Ok(client)
    }

    /// Observed and announced federations, each once
    async fn voted_federation_ids(&self) -> anyhow::Result<Vec<FederationId>> {
        self.connection()
            .await?
            .query(
                "SELECT federation_id FROM federations
                 UNION
                 SELECT federation_id FROM nostr_federations",
                &[],
            )
            .await?
            .iter()
            .map(|row| federation_id_from_db(&row.get::<_, Vec<u8>>(0)))
            .collect()
    }

    /// Stores the events not stored yet. Relays return every matching event
    /// on each request, so this skips the signature check and insert for the
    /// ones already known.
    async fn store_new_events<T: StoredEvent>(&self, events: Vec<Event>) -> anyhow::Result<()> {
        let mut conn = self.connection().await?;
        let event_ids = events
            .iter()
            .map(|event| event.id.to_bytes().to_vec())
            .collect::<Vec<_>>();
        let known = conn
            .query(
                &format!("SELECT event_id FROM {} WHERE event_id = ANY($1)", T::TABLE),
                &[&event_ids],
            )
            .await?
            .into_iter()
            .map(|row| row.get::<_, Vec<u8>>(0))
            .collect::<HashSet<_>>();

        let mut dbtx = conn.transaction().await?;
        let mut stored = 0;
        for (event, event_id) in events.iter().zip(event_ids) {
            if known.contains(&event_id) {
                continue;
            }
            // A failed statement aborts the whole transaction, so each event
            // gets a savepoint to fail alone
            let savepoint = dbtx.savepoint("nostr_event").await?;
            match async { T::parse(event)?.insert(&savepoint, event).await }.await {
                Ok(()) => {
                    savepoint.commit().await?;
                    stored += 1;
                }
                Err(e) => {
                    warn!(%e, "Failed to store {} {}", T::LABEL, event.id);
                    savepoint.rollback().await?;
                }
            }
        }
        dbtx.commit().await?;

        debug!(
            "Fetched {} {}s, stored {stored} new ones",
            events.len(),
            T::LABEL
        );
        Ok(())
    }

    pub async fn list_nostr_federations(&self) -> anyhow::Result<Vec<NostrFederation>> {
        #[derive(FromRow)]
        struct RawNostrFederation {
            federation_id: Vec<u8>,
            invite_code: String,
        }

        query::<RawNostrFederation>(
            &self.connection().await?,
            // language=postgresql
            "select federation_id, MIN(invite_code) as invite_code from nostr_federations group by federation_id",
            &[],
        )
        .await?
        .into_iter()
        .map(|federation| {
            let federation_id_bytes: [u8; 32] = federation
                .federation_id
                .try_into()
                .map_err(|_| anyhow!("Unexpected byte array len"))?;
            Ok(NostrFederation {
                federation_id: FederationId(bitcoin::hashes::sha256::Hash::from_byte_array(
                    federation_id_bytes,
                )),
                invite_code: InviteCode::from_str(&federation.invite_code)?,
            })
        })
        .collect()
    }

    /// Number and average of star votes of every federation that has votes
    pub async fn federation_ratings(
        &self,
    ) -> anyhow::Result<HashMap<FederationId, FederationRating>> {
        self.connection()
            .await?
            .query(
                // language=postgresql
                "SELECT federation_id, COUNT(star_vote)::bigint, AVG(star_vote)::DOUBLE PRECISION
                 FROM nostr_votes
                 GROUP BY federation_id",
                &[],
            )
            .await?
            .iter()
            .map(|row| {
                Ok((
                    federation_id_from_db(&row.get::<_, Vec<u8>>(0))?,
                    FederationRating {
                        count: row.get::<_, i64>(1) as u64,
                        avg: row.get(2),
                    },
                ))
            })
            .collect()
    }

    pub async fn submit_rating(&self, nostr_event: Event) -> anyhow::Result<()> {
        self.submit::<ParsedRecommendationEvent>(nostr_event).await
    }

    pub async fn submit_federation(&self, nostr_event: Event) -> anyhow::Result<()> {
        self.submit::<ParsedFederationEvent>(nostr_event).await
    }

    /// Checks an event, sends it to the relays and stores it
    async fn submit<T: StoredEvent>(&self, nostr_event: Event) -> anyhow::Result<()> {
        let parsed = T::parse(&nostr_event)?;

        self.nostr_relay_client()
            .await?
            .send_event(
                nostr_event.clone(),
                RelaySendOptions::default().timeout(Some(Duration::from_secs(5))),
            )
            .await?;

        parsed.insert(&self.connection().await?, &nostr_event).await
    }
}

/// A kind of Nostr event the observer stores
trait StoredEvent: Sized {
    const LABEL: &'static str;
    /// Table with an `event_id` column holding the stored events
    const TABLE: &'static str;

    /// Checks the event, including its signature: deserializing an event
    /// doesn't, so unchecked events could be stored and relayed in anyone's
    /// name
    fn parse(event: &Event) -> anyhow::Result<Self>;

    async fn insert(&self, client: &impl GenericClient, event: &Event) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct NostrFederation {
    pub federation_id: FederationId,
    pub invite_code: InviteCode,
}

#[derive(Debug, Clone)]
struct ParsedFederationEvent {
    event_id: [u8; 32],
    federation_id: FederationId,
    invite_code: InviteCode,
}

impl StoredEvent for ParsedFederationEvent {
    const LABEL: &'static str = "federation announcement";
    const TABLE: &'static str = "nostr_federations";

    fn parse(event: &Event) -> anyhow::Result<Self> {
        ensure!(
            event.kind == FEDERATION_ANNOUNCEMENT_EVENT_KIND,
            "Not a federation invite event"
        );
        event.verify().context("Invalid Nostr event signature")?;

        let federation_invite_tag = SingleLetterTag::from_char('u').expect("Tag is valid");
        let federation_id = tagged_federation_id(event)?;
        let invite_code = event
            .tags()
            .iter()
            .find_map(|tag| {
                if tag.single_letter_tag() != Some(federation_invite_tag) {
                    return None;
                }

                tag.as_vec().get(1)?.parse::<InviteCode>().ok()
            })
            .context("No invite code tag found")?;

        ensure!(
            invite_code.federation_id() == federation_id,
            "Federation id mismatch"
        );

        Ok(ParsedFederationEvent {
            event_id: event.id.to_bytes(),
            federation_id,
            invite_code,
        })
    }

    async fn insert(&self, client: &impl GenericClient, event: &Event) -> anyhow::Result<()> {
        debug!(
            "Inserting event {} for federation {}",
            hex::encode(self.event_id),
            self.federation_id
        );

        let now = chrono::Utc::now().naive_utc();
        client.execute(
            // language=postgresql
            "INSERT INTO nostr_federations (event_id, federation_id, invite_code, event, fetch_time) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            &[
                &self.event_id.to_vec(),
                &self.federation_id.consensus_encode_to_vec(),
                &self.invite_code.to_string(),
                &serde_json::to_value(event).expect("can be serialized"),
                &now
            ],
        ).await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ParsedRecommendationEvent {
    event_id: [u8; 32],
    federation_id: FederationId,
    star_vote: Option<u8>,
}

impl StoredEvent for ParsedRecommendationEvent {
    const LABEL: &'static str = "federation vote";
    const TABLE: &'static str = "nostr_votes";

    fn parse(event: &Event) -> anyhow::Result<Self> {
        ensure!(
            event.kind == RECOMMENDATION_EVENT_KIND,
            "Not a federation recommendation"
        );
        event.verify().context("Invalid Nostr event signature")?;

        let star_vote = extract_star_rating(&event.content);
        if let Some(star_vote) = star_vote {
            ensure!(star_vote <= 5, "Vote above 5 is invalid");
        }

        Ok(ParsedRecommendationEvent {
            event_id: event.id.to_bytes(),
            federation_id: tagged_federation_id(event)?,
            star_vote,
        })
    }

    async fn insert(&self, client: &impl GenericClient, event: &Event) -> anyhow::Result<()> {
        debug!(
            "Inserting event {} for federation {}",
            hex::encode(self.event_id),
            self.federation_id
        );

        let now = chrono::Utc::now().naive_utc();
        client.execute(
            // language=postgresql
            "INSERT INTO nostr_votes (event_id, federation_id, star_vote, event, fetch_time) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            &[
                &self.event_id.to_vec(),
                &self.federation_id.consensus_encode_to_vec(),
                &self.star_vote.map(|vote| vote as i32),
                &serde_json::to_value(event).expect("can be serialized"),
                &now
            ],
        ).await?;

        Ok(())
    }
}

fn tagged_federation_id(event: &Event) -> anyhow::Result<FederationId> {
    let federation_tag = federation_id_tag();
    event
        .tags()
        .iter()
        .find_map(|tag| {
            if tag.single_letter_tag() != Some(federation_tag) {
                return None;
            }

            tag.as_vec().get(1)?.parse::<FederationId>().ok()
        })
        .context("No federation id tag found")
}

fn since_filter(filter: Filter, since: Option<Timestamp>) -> Filter {
    match since {
        Some(since) => filter.since(since),
        None => filter,
    }
}

fn votes_filter(
    federations: impl IntoIterator<Item = FederationId>,
    since: Option<Timestamp>,
) -> Filter {
    since_filter(
        Filter::new().kind(RECOMMENDATION_EVENT_KIND).custom_tag(
            federation_id_tag(),
            federations
                .into_iter()
                .map(|federation_id| federation_id.to_string()),
        ),
        since,
    )
}

async fn fetch_events(client: &RelayPool, filters: Vec<Filter>) -> anyhow::Result<Vec<Event>> {
    Ok(client
        .get_events_of(filters, NOSTR_FETCH_TIMEOUT, FilterOptions::default())
        .await?)
}

fn extract_star_rating(comment: &str) -> Option<u8> {
    static STAR_RATING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\[([0-9]+)/5]").expect("valid regex"));
    let rating = STAR_RATING
        .captures(comment)?
        .get(1)?
        .as_str()
        .parse::<u8>()
        .ok()?;

    if (1..=5).contains(&rating) {
        Some(rating)
    } else {
        None
    }
}

pub(crate) async fn get_nostr_federations(
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<BTreeMap<FederationId, InviteCode>>> {
    let federation_map = state
        .federation_observer
        .list_nostr_federations()
        .await?
        .into_iter()
        .map(|federation| (federation.federation_id, federation.invite_code))
        .collect();

    Ok(Json(federation_map))
}

pub(crate) async fn publish_federation_event(
    State(state): State<AppState>,
    Json(event): Json<nostr_sdk::Event>,
) -> crate::error::Result<()> {
    Ok(state.federation_observer.submit_federation(event).await?)
}

#[cfg(test)]
mod tests {
    use fedimint_core::config::FederationId;
    use nostr_sdk::{EventBuilder, Keys, Tag};

    use super::{ParsedRecommendationEvent, StoredEvent, RECOMMENDATION_EVENT_KIND};

    #[test]
    fn only_correctly_signed_ratings_are_accepted() {
        let federation_id = FederationId::dummy();
        let event = EventBuilder::new(
            RECOMMENDATION_EVENT_KIND,
            "[5/5] great",
            [Tag::identifier(federation_id.to_string())],
        )
        .to_event(&Keys::generate())
        .unwrap();

        let parsed = ParsedRecommendationEvent::parse(&event).unwrap();
        assert_eq!(parsed.federation_id, federation_id);
        assert_eq!(parsed.star_vote, Some(5));

        // What a PUT body can carry: the signed event with its content edited.
        // Deserializing it succeeds, so the signature check must catch it.
        let mut json = serde_json::to_value(&event).unwrap();
        json["content"] = "[1/5] terrible".into();
        let forged: nostr_sdk::Event = serde_json::from_value(json).unwrap();
        assert!(ParsedRecommendationEvent::parse(&forged).is_err());
    }
}
