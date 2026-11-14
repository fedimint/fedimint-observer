//! Data sources: how information that doesn't come from a federation's
//! guardians gets into the observer.
//!
//! A source is anything that observes federations from the outside: an
//! external observability service, a directory, a probe network. It declares
//! the *kinds* of observation it produces up front ([`SourceKind`]), and core
//! stores the newest observation of each kind per federation in
//! `source_observations`. Sources feed the observer in one of two ways:
//!
//! - **Pull**, compiled in: an [`ObservationSource`] whose
//!   [`task`](ObservationSource::task) collects and writes through the
//!   [`ObservationSink`] it is given. Added with
//!   `FedimintObserver::builder().with_source(...)`.
//! - **Push**, over HTTP: a service outside the build sends `PUT
//!   /sources/:source/federations/:federation_id/:kind` with a bearer token.
//!   Push sources are declared in the `FO_SOURCES_FILE` JSON document, each
//!   with its token in `FO_SOURCE_TOKEN_<ID>` (see [`PushSource`]).
//!
//! Whichever way it arrives, every observation goes through the same checks
//! before it is stored: the kind must be declared, the data a JSON object
//! within the kind's size and depth limits, the observation time neither in
//! the future nor past the kind's retention, and the federation observed. A
//! write older than the stored observation is ignored, so retries and
//! reordered deliveries can't replace newer data. Each source has its own
//! write concurrency, so one source can't take the database pool.
//!
//! Kinds declared `public` are served by `GET /federations/:id/sources`
//! verbatim. Core can check the shape of the data but not its meaning: a
//! source must only publish federation-level figures, never anything that
//! points at one user's or one operator's funds or payments. Kinds that are
//! not public are kept for the build's own code
//! ([`Services::source_observation`]).

mod api;
mod config;
mod sink;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bitcoin::hashes::{sha256, Hash};
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tracing::{info_span, Instrument};

pub(crate) use self::api::{federations_routes, routes};
pub(crate) use self::config::load_push_sources;
pub use self::config::PushSource;
pub use self::sink::{ObservationSink, Recorded};
use crate::federation::observer::FederationObserver;
use crate::module::{Services, Task};
use crate::task::supervise;

/// Upper bound of any kind's `max_bytes`: observations are summaries, not
/// datasets
pub const MAX_OBSERVATION_BYTES: usize = 64 * 1024;
/// Nesting depth of an observation's JSON, which bounds the work of storing
/// and serving it
pub const MAX_JSON_DEPTH: usize = 16;
/// How far in the future an observation's time may be, for sources whose
/// clocks are a little ahead
pub const MAX_CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);

const DEFAULT_MAX_BYTES: usize = 4 * 1024;
const DEFAULT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MIN_RETENTION: Duration = Duration::from_secs(60 * 60);
const MAX_RETENTION: Duration = Duration::from_secs(400 * 24 * 60 * 60);
/// Sources and kinds per source a build may declare. Bounds the table (one
/// row per federation and kind) and the label sets of the metrics.
const MAX_SOURCES: usize = 64;
const MAX_KINDS_PER_SOURCE: usize = 64;
/// Concurrent writes per source
const WRITE_CONCURRENCY: usize = 4;
/// How often observations past their retention are deleted
const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Rows of sources or kinds the build no longer declares are deleted this
/// long after they were last written, so a source left out of a deploy by
/// mistake keeps its data
const UNDECLARED_GRACE_DAYS: i32 = 30;

/// One kind of observation a source produces, such as `probe_latency`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKind {
    name: String,
    max_bytes: usize,
    public: bool,
    retention: Duration,
}

impl SourceKind {
    /// A kind that is not public, of at most 4 KiB, kept for 7 days. Names
    /// are 1 to 64 of `a-z0-9_.-`, starting with a letter.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            public: false,
            retention: DEFAULT_RETENTION,
        }
    }

    /// Serve this kind on `GET /federations/:id/sources`. Only for
    /// federation-level data: see the module documentation.
    pub fn public(mut self) -> Self {
        self.public = true;
        self
    }

    /// The largest observation, as compact JSON, at most
    /// [`MAX_OBSERVATION_BYTES`]
    pub fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// How long an observation stays after the time it was made: it is no
    /// longer served past it and is deleted within the hour. Between an hour
    /// and 400 days.
    pub fn retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_public(&self) -> bool {
        self.public
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            valid_name(&self.name, 64, |byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            }),
            "Source kind {:?} is not 1 to 64 of a-z0-9_.- starting with a letter",
            self.name
        );
        anyhow::ensure!(
            (1..=MAX_OBSERVATION_BYTES).contains(&self.max_bytes),
            "Source kind {} allows {} bytes; the limit is 1 to {MAX_OBSERVATION_BYTES}",
            self.name,
            self.max_bytes
        );
        anyhow::ensure!(
            (MIN_RETENTION..=MAX_RETENTION).contains(&self.retention),
            "Source kind {} keeps observations for {:?}; the limit is an hour to 400 days",
            self.name,
            self.retention
        );
        Ok(())
    }

    /// Whether an observation made at `observed_at` is past its retention
    fn expired(&self, observed_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(observed_at)
            .to_std()
            .is_ok_and(|age| age > self.retention)
    }
}

/// What a source's task gets: core's services, and the sink it writes its
/// observations through
#[derive(Clone)]
pub struct SourceContext {
    pub services: Services,
    pub sink: ObservationSink,
}

/// A source of observations about federations that isn't one of their
/// guardians. See the module documentation.
pub trait ObservationSource: Send + Sync + 'static {
    /// Names the source in URLs, metrics and the database: 1 to 32 of
    /// `a-z0-9_`, starting with a letter. Must stay the same across releases,
    /// since stored observations are keyed by it.
    fn id(&self) -> &str;

    /// Every kind of observation the source produces. Writes of other kinds
    /// are rejected.
    fn kinds(&self) -> Vec<SourceKind>;

    /// The source's collector, if it pulls. Core runs it in the worker,
    /// supervised like module tasks: 30 s after it ends or fails, it asks for
    /// the task again, so this must answer the same every time. Calls to
    /// external services should hold [`Services::external_io_permit`].
    fn task(&self, _ctx: &SourceContext) -> Option<Task> {
        None
    }

    /// The token services must present to push this source's observations
    /// over HTTP, if it accepts pushes
    fn push_token(&self) -> Option<&PushToken> {
        None
    }
}

/// A bearer token for pushing observations. Only its SHA-256 is kept, and it
/// is compared in constant time.
#[derive(Clone)]
pub struct PushToken([u8; 32]);

impl PushToken {
    /// Tokens are 32 to 512 bytes; generate one with `openssl rand -hex 32`
    pub fn new(secret: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (32..=512).contains(&secret.len()),
            "Push tokens must be 32 to 512 bytes long"
        );
        Ok(Self(sha256::Hash::hash(secret.as_bytes()).to_byte_array()))
    }

    pub(crate) fn verify(&self, presented: &str) -> bool {
        let presented = sha256::Hash::hash(presented.as_bytes()).to_byte_array();
        bool::from(self.0.ct_eq(&presented))
    }
}

impl std::fmt::Debug for PushToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushToken(<redacted>)")
    }
}

/// Counters of a source's writes, for `/metrics`
#[derive(Debug, Default)]
struct SourceMetrics {
    stored: AtomicU64,
    superseded: AtomicU64,
    rejected: AtomicU64,
}

pub(crate) struct RegisteredSource {
    id: String,
    source: Arc<dyn ObservationSource>,
    kinds: BTreeMap<String, SourceKind>,
    writes: Semaphore,
    metrics: SourceMetrics,
}

impl RegisteredSource {
    fn kind(&self, name: &str) -> Option<&SourceKind> {
        self.kinds.get(name)
    }
}

/// The sources an observer build includes, by id
#[derive(Clone, Default)]
pub struct SourceRegistry(Arc<BTreeMap<String, Arc<RegisteredSource>>>);

impl std::fmt::Debug for SourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.keys()).finish()
    }
}

impl SourceRegistry {
    pub(crate) fn new(sources: Vec<Arc<dyn ObservationSource>>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            sources.len() <= MAX_SOURCES,
            "A build has at most {MAX_SOURCES} sources"
        );
        let mut by_id = BTreeMap::new();
        for source in sources {
            let id = source.id().to_owned();
            anyhow::ensure!(
                valid_name(&id, 32, |byte| byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || byte == b'_'),
                "Source id {id:?} is not 1 to 32 of a-z0-9_ starting with a letter"
            );
            let declared = source.kinds();
            anyhow::ensure!(
                !declared.is_empty() && declared.len() <= MAX_KINDS_PER_SOURCE,
                "Source {id} declares {} kinds; it needs 1 to {MAX_KINDS_PER_SOURCE}",
                declared.len()
            );
            let mut kinds = BTreeMap::new();
            for kind in declared {
                kind.validate()?;
                let name = kind.name.clone();
                anyhow::ensure!(
                    kinds.insert(name.clone(), kind).is_none(),
                    "Source {id} declares kind {name} twice"
                );
            }
            let registered = Arc::new(RegisteredSource {
                id: id.clone(),
                source,
                kinds,
                writes: Semaphore::new(WRITE_CONCURRENCY),
                metrics: SourceMetrics::default(),
            });
            anyhow::ensure!(
                by_id.insert(id.clone(), registered).is_none(),
                "Source {id} was added twice"
            );
        }
        Ok(Self(Arc::new(by_id)))
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    fn get(&self, id: &str) -> Option<&Arc<RegisteredSource>> {
        self.0.get(id)
    }

    fn iter(&self) -> impl Iterator<Item = &Arc<RegisteredSource>> {
        self.0.values()
    }

    /// The source's declared kind, if both exist
    fn kind(&self, source: &str, kind: &str) -> Option<&SourceKind> {
        self.get(source)?.kind(kind)
    }

    /// Appends the write counters of every source in Prometheus text format
    pub(crate) fn render_metrics(&self, body: &mut String) {
        if self.0.is_empty() {
            return;
        }
        body.push_str("# TYPE fmo_source_observations_total counter\n");
        for source in self.iter() {
            for (result, counter) in [
                ("stored", &source.metrics.stored),
                ("superseded", &source.metrics.superseded),
                ("rejected", &source.metrics.rejected),
            ] {
                // Ids are validated to a-z0-9_, so they need no escaping
                let _ = writeln!(
                    body,
                    "fmo_source_observations_total{{source=\"{}\",result=\"{result}\"}} {}",
                    source.id,
                    counter.load(Ordering::Relaxed)
                );
            }
        }
    }
}

fn valid_name(name: &str, max_len: usize, allowed: impl Fn(u8) -> bool) -> bool {
    (1..=max_len).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name.bytes().all(allowed)
}

impl FederationObserver {
    /// Runs each pulling source's task and the retention of their
    /// observations. Worker only.
    pub(crate) fn spawn_source_tasks(&self) {
        let sources = self.sources().clone();
        if sources.0.is_empty() {
            return;
        }
        let services = Services::new(self.clone());
        for source in sources.iter() {
            let ctx = SourceContext {
                services: services.clone(),
                sink: ObservationSink::new(source.clone(), services.clone()),
            };
            if source.source.task(&ctx).is_none() {
                continue;
            }
            let id = source.id.clone();
            let source = source.source.clone();
            self.task_group().spawn_cancellable(
                format!("{id} source task"),
                supervise(format!("{id} source task"), move || {
                    source.task(&ctx).expect("the source had a task")
                })
                .instrument(info_span!("source", id = %id)),
            );
        }

        let observer = self.clone();
        self.task_group().spawn_cancellable(
            "source retention",
            supervise("Source retention", move || {
                let observer = observer.clone();
                let sources = sources.clone();
                async move {
                    loop {
                        let deleted = sink::prune(&*observer.connection().await?, &sources).await?;
                        if deleted > 0 {
                            tracing::info!(deleted, "Deleted expired source observations");
                        }
                        tokio::time::sleep(PRUNE_INTERVAL).await;
                    }
                }
            }),
        );
    }
}

impl Services {
    /// The newest observation of a source's kind about a federation, public
    /// or not, with the time it was made. `None` when there is none, or it is
    /// past the kind's retention, or the build doesn't declare the kind.
    pub async fn source_observation(
        &self,
        federation_id: FederationId,
        source: &str,
        kind: &str,
    ) -> anyhow::Result<Option<(Value, DateTime<Utc>)>> {
        let Some(declared) = self.sources().kind(source, kind) else {
            return Ok(None);
        };
        let row = self
            .connection()
            .await?
            .query_opt(
                "SELECT data, observed_at FROM source_observations
                 WHERE federation_id = $1 AND source = $2 AND kind = $3",
                &[&federation_id.consensus_encode_to_vec(), &source, &kind],
            )
            .await?;
        Ok(row
            .map(|row| (row.get::<_, Value>(0), row.get::<_, DateTime<Utc>>(1)))
            .filter(|(_, observed_at)| !declared.expired(*observed_at, Utc::now())))
    }
}
