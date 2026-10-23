use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, ensure, Context};
use axum::extract::State;
use axum::Json;
use deadpool_postgres::GenericClient;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::task::sleep;
use fedimint_core::BitcoinHash;
use fmo_api_types::FederationRating;
use nostr_sdk::{
    Event, Filter, FilterOptions, Kind, RelayOptions, RelayPool, RelayPoolOptions,
    RelaySendOptions, SingleLetterTag,
};
use postgres_from_row::FromRow;
use regex::Regex;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::federation::observer::FederationObserver;
use crate::util::{query, query_one};
use crate::AppState;

// TODO: move to common crate
const FEDERATION_ANNOUNCEMENT_EVENT_KIND: Kind = Kind::Custom(38173);
const RECOMMENDATION_EVENT_KIND: Kind = Kind::Custom(38000);

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
    /// observed federations
    pub async fn sync_nostr_events(self) {
        const SLEEP_SECS: u64 = 60;
        loop {
            match self.sync_nostr_events_inner().await {
                Ok(()) => warn!("Nostr sync exited unexpectedly"),
                Err(e) => warn!("Error while syncing nostr events: {e:?}"),
            }
            sleep(Duration::from_secs(SLEEP_SECS)).await;
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

    async fn sync_nostr_events_inner(&self) -> anyhow::Result<()> {
        let mut interval = interval(Duration::from_secs(60));

        let client = self.nostr_relay_client().await?;

        loop {
            interval.tick().await;

            self.sync_federation_announcements(&client).await?;

            let federations = {
                let observed_federations = self.list_federations().await?;
                let nostr_federations = self.list_nostr_federations().await?;
                observed_federations
                    .into_iter()
                    .map(|federation| federation.federation_id)
                    .chain(
                        nostr_federations
                            .into_iter()
                            .map(|federation| federation.federation_id),
                    )
                    .collect()
            };

            self.sync_federation_votes(&client, federations).await?;
        }
    }

    async fn sync_federation_votes(
        &self,
        client: &RelayPool,
        federations: Vec<FederationId>,
    ) -> anyhow::Result<()> {
        for federation_id in federations {
            let events = fetch_federation_votes(client, federation_id).await?;

            debug!(
                "Fetched {} votes for federation {}",
                events.len(),
                federation_id
            );
            let mut conn = self.connection().await?;
            let mut dbtx = conn.transaction().await?;
            for event in events {
                let event_id = event.id;
                // A failed statement aborts the whole transaction, so each
                // event gets a savepoint to fail alone
                let savepoint = dbtx.savepoint("nostr_vote").await?;
                match insert_federation_votes(&savepoint, event).await {
                    Ok(()) => savepoint.commit().await?,
                    Err(e) => {
                        warn!(%e, "Failed to insert federation vote {}", event_id);
                        savepoint.rollback().await?;
                    }
                }
            }
            dbtx.commit().await?;
        }

        Ok(())
    }

    async fn sync_federation_announcements(&self, client: &RelayPool) -> anyhow::Result<()> {
        let events = fetch_federations(client).await?;

        debug!("Fetched {} federation announcements", events.len());

        let mut conn = self.connection().await?;
        let mut dbtx = conn.transaction().await?;
        for event in events {
            let event_id = event.id;
            let savepoint = dbtx.savepoint("nostr_announcement").await?;
            match insert_federation(&savepoint, event).await {
                Ok(()) => savepoint.commit().await?,
                Err(e) => {
                    warn!(%e, "Failed to insert federation announcement {}", event_id);
                    savepoint.rollback().await?;
                }
            }
        }
        dbtx.commit().await?;

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

    pub async fn federation_rating(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<FederationRating> {
        #[derive(Debug, Clone, FromRow)]
        struct FederationRatingRow {
            count: i64,
            avg: Option<f64>,
        }

        let query_res = query_one::<FederationRatingRow>(
            &self.connection().await?,
            // language=postgresql
            "SELECT COUNT(star_vote)::bigint as count, AVG(star_vote)::DOUBLE PRECISION as avg from nostr_votes WHERE federation_id = $1;",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        Ok(FederationRating {
            count: query_res.count as u64,
            avg: query_res.avg,
        })
    }

    pub async fn submit_rating(&self, nostr_event: Event) -> anyhow::Result<()> {
        ParsedRecommendationEvent::try_from(nostr_event.clone())?;

        let client = self.nostr_relay_client().await?;

        client
            .send_event(
                nostr_event.clone(),
                RelaySendOptions::default().timeout(Some(Duration::from_secs(5))),
            )
            .await?;

        let mut conn = self.connection().await?;
        let dbtx = conn.transaction().await?;
        insert_federation_votes(&dbtx, nostr_event).await?;
        dbtx.commit().await?;

        Ok(())
    }

    // TODO: deduplicate with submit_rating, make nostr stuff its own service
    pub async fn submit_federation(&self, nostr_event: Event) -> anyhow::Result<()> {
        ParsedFederationEvent::try_from(nostr_event.clone())?;

        let client = self.nostr_relay_client().await?;

        client
            .send_event(
                nostr_event.clone(),
                RelaySendOptions::default().timeout(Some(Duration::from_secs(5))),
            )
            .await?;

        let mut conn = self.connection().await?;
        let dbtx = conn.transaction().await?;
        insert_federation(&dbtx, nostr_event).await?;
        dbtx.commit().await?;

        Ok(())
    }
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

impl TryFrom<Event> for ParsedFederationEvent {
    type Error = anyhow::Error;

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        ensure!(
            event.kind == FEDERATION_ANNOUNCEMENT_EVENT_KIND,
            "Not a federation invite event"
        );
        // Deserializing an event doesn't check it, so unverified events
        // could be stored and relayed in anyone's name
        event.verify().context("Invalid Nostr event signature")?;

        let event_id = event.id.to_bytes();

        let federation_invite_tag = SingleLetterTag::from_char('u').expect("Tag is valid");
        let id_tag = federation_id_tag();

        let federation_id = event
            .tags()
            .iter()
            .find_map(|tag| {
                if tag.single_letter_tag() != Some(id_tag) {
                    return None;
                }

                tag.as_vec().get(1)?.parse::<FederationId>().ok()
            })
            .context("No federation id tag found")?;

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
            event_id,
            federation_id,
            invite_code,
        })
    }
}

#[derive(Debug, Clone)]
struct ParsedRecommendationEvent {
    event_id: [u8; 32],
    federation_id: FederationId,
    star_vote: Option<u8>,
}

impl TryFrom<Event> for ParsedRecommendationEvent {
    type Error = anyhow::Error;

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        ensure!(
            event.kind == RECOMMENDATION_EVENT_KIND,
            "Not a federation recommendation"
        );
        event.verify().context("Invalid Nostr event signature")?;

        let federation_tag = federation_id_tag();

        let event_id = event.id.to_bytes();

        let federation_id = event
            .tags()
            .iter()
            .find_map(|tag| {
                if tag.single_letter_tag() != Some(federation_tag) {
                    return None;
                }

                tag.as_vec().get(1)?.parse::<FederationId>().ok()
            })
            .context("No federation id tag found")?;

        let star_vote = extract_star_rating(&event.content);

        if let Some(star_vote) = star_vote {
            ensure!(star_vote <= 5, "Vote above 5 is invalid");
        }

        Ok(ParsedRecommendationEvent {
            event_id,
            federation_id,
            star_vote,
        })
    }
}

async fn fetch_federations(client: &RelayPool) -> anyhow::Result<Vec<Event>> {
    let events = client
        .get_events_of(
            vec![Filter {
                kinds: Some(
                    vec![FEDERATION_ANNOUNCEMENT_EVENT_KIND]
                        .into_iter()
                        .collect(),
                ),
                ..Filter::new()
            }],
            Duration::from_secs(30),
            FilterOptions::default(),
        )
        .await?;

    Ok(events)
}

async fn insert_federation(
    dbtx: &deadpool_postgres::Transaction<'_>,
    event: Event,
) -> anyhow::Result<()> {
    let parsed_event = ParsedFederationEvent::try_from(event.clone())?;

    debug!(
        "Inserting event {} for federation {}",
        hex::encode(parsed_event.event_id),
        parsed_event.federation_id
    );

    let now = chrono::Utc::now().naive_utc();
    dbtx.execute(
        // language=postgresql
        "INSERT INTO nostr_federations (event_id, federation_id, invite_code, event, fetch_time) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &parsed_event.event_id.to_vec(),
            &parsed_event.federation_id.consensus_encode_to_vec(),
            &parsed_event.invite_code.to_string(),
            &serde_json::to_value(event).expect("can be serialized"),
            &now
        ],
    ).await?;

    Ok(())
}

async fn fetch_federation_votes(
    client: &RelayPool,
    federation_id: FederationId,
) -> anyhow::Result<Vec<Event>> {
    let federation_tag = federation_id_tag();

    let events = client
        .get_events_of(
            vec![Filter {
                kinds: Some(vec![RECOMMENDATION_EVENT_KIND].into_iter().collect()),
                generic_tags: HashMap::from([(
                    federation_tag,
                    HashSet::from([federation_id.to_string()]),
                )]),
                ..Filter::new()
            }],
            Duration::from_secs(30),
            FilterOptions::default(),
        )
        .await?;

    Ok(events)
}

async fn insert_federation_votes(
    dbtx: &deadpool_postgres::Transaction<'_>,
    event: Event,
) -> anyhow::Result<()> {
    let parsed_event = ParsedRecommendationEvent::try_from(event.clone())?;

    debug!(
        "Inserting event {} for federation {}",
        hex::encode(parsed_event.event_id),
        parsed_event.federation_id
    );

    let now = chrono::Utc::now().naive_utc();
    dbtx.execute(
        // language=postgresql
        "INSERT INTO nostr_votes (event_id, federation_id, star_vote, event, fetch_time) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &parsed_event.event_id.to_vec(),
            &parsed_event.federation_id.consensus_encode_to_vec(),
            &parsed_event.star_vote.map(|vote| vote as i32),
            &serde_json::to_value(event).expect("can be serialized"),
            &now
        ],
    ).await?;

    Ok(())
}

fn extract_star_rating(comment: &str) -> Option<u8> {
    let re = Regex::new(r"^\[([0-9]+)/5]").expect("valid regex");
    let rating = re.captures(comment)?.get(1)?.as_str().parse::<u8>().ok()?;

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

    use super::{ParsedRecommendationEvent, RECOMMENDATION_EVENT_KIND};

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

        let parsed = ParsedRecommendationEvent::try_from(event.clone()).unwrap();
        assert_eq!(parsed.federation_id, federation_id);
        assert_eq!(parsed.star_vote, Some(5));

        // What a PUT body can carry: the signed event with its content edited.
        // Deserializing it succeeds, so the signature check must catch it.
        let mut json = serde_json::to_value(&event).unwrap();
        json["content"] = "[1/5] terrible".into();
        let forged: nostr_sdk::Event = serde_json::from_value(json).unwrap();
        assert!(ParsedRecommendationEvent::try_from(forged).is_err());
    }
}
