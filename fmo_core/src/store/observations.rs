//! Per-federation observations: persisted, last-known-good snapshots that
//! collectors write (metadata, module asset totals) and readers serve. A
//! failed poll never replaces good data.

use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use serde_json::Value;

use crate::module::Services;

impl Services {
    pub async fn save_observation(
        &self,
        federation_id: FederationId,
        kind: &str,
        data: &Value,
    ) -> anyhow::Result<()> {
        self.connection().await?.execute(
            "INSERT INTO federation_observations(federation_id, kind, data) VALUES ($1, $2, $3)
             ON CONFLICT (federation_id, kind) DO UPDATE SET data = EXCLUDED.data, observed_at = now()",
            &[&federation_id.consensus_encode_to_vec(), &kind, data],
        ).await?;
        Ok(())
    }

    pub async fn observation(
        &self,
        federation_id: FederationId,
        kind: &str,
    ) -> anyhow::Result<Option<(Value, DateTime<Utc>)>> {
        Ok(self.connection().await?.query_opt(
            "SELECT data, observed_at FROM federation_observations WHERE federation_id = $1 AND kind = $2",
            &[&federation_id.consensus_encode_to_vec(), &kind],
        ).await?.map(|row| (row.get(0), row.get(1))))
    }
}
