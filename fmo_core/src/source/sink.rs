//! The one path by which source observations are stored: validation, the
//! newest-wins upsert, and retention.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use serde_json::Value;

use super::{
    RegisteredSource, SourceKind, SourceRegistry, MAX_CLOCK_SKEW, MAX_JSON_DEPTH,
    UNDECLARED_GRACE_DAYS,
};
use crate::error::ClientError;
use crate::module::Services;

/// What became of a valid observation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    /// It is now the newest of its kind for the federation
    Stored,
    /// One made at the same time or later was already stored; nothing changed
    Superseded,
}

/// Writes one source's observations. Each source gets its own, bound to the
/// kinds it declared.
#[derive(Clone)]
pub struct ObservationSink {
    source: Arc<RegisteredSource>,
    services: Services,
}

impl ObservationSink {
    pub(super) fn new(source: Arc<RegisteredSource>, services: Services) -> Self {
        Self { source, services }
    }

    pub fn source_id(&self) -> &str {
        &self.source.id
    }

    /// Stores an observation of `kind` about the federation, made at
    /// `observed_at`, unless a newer one is stored. Waits while the source
    /// has as many writes in flight as it may.
    ///
    /// Fails with a [`ClientError`] if the observation is invalid (see the
    /// module documentation) or the federation is not observed.
    pub async fn record(
        &self,
        federation_id: FederationId,
        kind: &str,
        observed_at: DateTime<Utc>,
        data: &Value,
    ) -> anyhow::Result<Recorded> {
        let _permit = self.source.writes.acquire().await?;
        self.store(federation_id, kind, observed_at, data).await
    }

    /// Like [`Self::record`], but fails at once with a 429 when the source
    /// has as many writes in flight as it may: for requests, which must not
    /// queue
    pub(super) async fn try_record(
        &self,
        federation_id: FederationId,
        kind: &str,
        observed_at: DateTime<Utc>,
        data: &Value,
    ) -> anyhow::Result<Recorded> {
        let Ok(_permit) = self.source.writes.try_acquire() else {
            self.source.metrics.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(ClientError::too_many_requests(format!(
                "Source {} has too many writes in flight",
                self.source.id
            ))
            .into());
        };
        self.store(federation_id, kind, observed_at, data).await
    }

    async fn store(
        &self,
        federation_id: FederationId,
        kind: &str,
        observed_at: DateTime<Utc>,
        data: &Value,
    ) -> anyhow::Result<Recorded> {
        let metrics = &self.source.metrics;
        let checked = async {
            validate(self.source.kind(kind), kind, observed_at, data, Utc::now())?;
            // Before storing anything for the id, so unknown ids cost no rows
            self.services.ensure_observed(federation_id).await
        }
        .await;
        if let Err(error) = checked {
            metrics.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        let recorded = upsert(
            &*self.services.connection().await?,
            federation_id,
            &self.source.id,
            kind,
            observed_at,
            data,
        )
        .await?;
        match recorded {
            Recorded::Stored => &metrics.stored,
            Recorded::Superseded => &metrics.superseded,
        }
        .fetch_add(1, Ordering::Relaxed);
        Ok(recorded)
    }
}

/// Checks an observation against its declared kind (`None`: not declared)
pub(super) fn validate(
    declared: Option<&SourceKind>,
    kind: &str,
    observed_at: DateTime<Utc>,
    data: &Value,
    now: DateTime<Utc>,
) -> Result<(), ClientError> {
    let Some(declared) = declared else {
        return Err(ClientError::bad_request(format!(
            "The source declares no kind {kind:?}"
        )));
    };
    if !data.is_object() {
        return Err(ClientError::bad_request(
            "Observation data must be a JSON object",
        ));
    }
    if json_depth_exceeds(data, MAX_JSON_DEPTH) {
        return Err(ClientError::bad_request(format!(
            "Observation data nests deeper than {MAX_JSON_DEPTH} levels"
        )));
    }
    // Serializing a Value can't fail
    let size = serde_json::to_vec(data).map_or(usize::MAX, |json| json.len());
    if size > declared.max_bytes {
        return Err(ClientError::bad_request(format!(
            "Observation data is {size} bytes; kind {kind} allows {}",
            declared.max_bytes
        )));
    }
    let skew = chrono::Duration::from_std(MAX_CLOCK_SKEW).expect("small duration");
    if observed_at > now + skew {
        return Err(ClientError::bad_request(
            "observed_at is in the future; check the source's clock",
        ));
    }
    if declared.expired(observed_at, now) {
        return Err(ClientError::bad_request(format!(
            "observed_at is older than kind {kind}'s retention"
        )));
    }
    Ok(())
}

/// Whether the JSON nests deeper than `max` arrays and objects. Iterative, so
/// hostile input can't overflow the stack.
fn json_depth_exceeds(value: &Value, max: usize) -> bool {
    let mut stack = vec![(value, 1)];
    while let Some((value, depth)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match value {
            Value::Array(items) => Box::new(items.iter()),
            Value::Object(fields) => Box::new(fields.values()),
            _ => continue,
        };
        if depth > max {
            return true;
        }
        stack.extend(children.map(|child| (child, depth + 1)));
    }
    false
}

/// Stores the observation unless one made at the same time or later is
/// stored. Same-time writes are idempotent retries.
pub(super) async fn upsert(
    client: &tokio_postgres::Client,
    federation_id: FederationId,
    source: &str,
    kind: &str,
    observed_at: DateTime<Utc>,
    data: &Value,
) -> anyhow::Result<Recorded> {
    let changed = client
        .execute(
            "INSERT INTO source_observations (federation_id, source, kind, data, observed_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (federation_id, source, kind) DO UPDATE
                 SET data = EXCLUDED.data, observed_at = EXCLUDED.observed_at, received_at = now()
                 WHERE source_observations.observed_at < EXCLUDED.observed_at",
            &[
                &federation_id.consensus_encode_to_vec(),
                &source,
                &kind,
                data,
                &observed_at,
            ],
        )
        .await?;
    Ok(if changed == 1 {
        Recorded::Stored
    } else {
        Recorded::Superseded
    })
}

/// Deletes observations past their kind's retention, and those of sources or
/// kinds the build doesn't declare that weren't written for
/// [`UNDECLARED_GRACE_DAYS`]. Returns how many it deleted. The table holds one
/// row per federation and kind, so one statement per kind stays small.
pub(super) async fn prune(
    client: &tokio_postgres::Client,
    sources: &SourceRegistry,
) -> anyhow::Result<u64> {
    let mut deleted = 0;
    let mut declared_sources = Vec::new();
    let mut declared_kinds = Vec::new();
    for source in sources.iter() {
        for kind in source.kinds.values() {
            deleted += client
                .execute(
                    "DELETE FROM source_observations
                     WHERE source = $1 AND kind = $2
                       AND observed_at < now() - make_interval(secs => $3)",
                    &[&source.id, &kind.name, &kind.retention.as_secs_f64()],
                )
                .await?;
            declared_sources.push(source.id.as_str());
            declared_kinds.push(kind.name.as_str());
        }
    }
    deleted += client
        .execute(
            "DELETE FROM source_observations o
             WHERE o.received_at < now() - make_interval(days => $3)
               AND NOT EXISTS (
                   SELECT 1 FROM unnest($1::text[], $2::text[]) AS d(source, kind)
                   WHERE d.source = o.source AND d.kind = o.kind
               )",
            &[&declared_sources, &declared_kinds, &UNDECLARED_GRACE_DAYS],
        )
        .await?;
    Ok(deleted)
}
