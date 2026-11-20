//! The read side of the federations list and totals: built from read models
//! and snapshots kept current as data is written, so no request aggregates
//! the full history.

use std::collections::HashMap;

use anyhow::Context;
use chrono::NaiveDate;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::Amount;
use fmo_api_types::{
    FederationActivity, FederationHealth, FederationRating, FederationSummary, FedimintTotals,
};
use postgres_from_row::FromRow;

use crate::config::meta::{MetaFields, MetaFieldsExt};
use crate::federation::observer::FederationObserver;
use crate::module::{Federation, Services};
use crate::store::rows::federation_id_from_db;
use crate::util::{query, query_one, query_value};

const NO_ACTIVITY: FederationActivity = FederationActivity {
    num_transactions: 0,
    amount_transferred: Amount::ZERO,
};

impl FederationObserver {
    /// The federation list uses a fixed number of database queries and no
    /// remote I/O.
    pub async fn list_federation_summaries(&self) -> anyhow::Result<Vec<FederationSummary>> {
        let federations = self.list_federations().await?;
        let federation_health = self.get_guardian_health_summary().await?;
        let assets = self.federation_assets().await?;
        let snapshots = self.asset_snapshots().await?;
        let mut activity = self.federations_activity(7).await?;
        let mut ratings = self.federation_ratings().await?;

        let mut metas = self
            .connection()
            .await?
            .query(
                "SELECT federation_id, data FROM federation_observations WHERE kind = 'meta'",
                &[],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok((
                    federation_id_from_db(&row.get::<_, Vec<u8>>(0))?,
                    serde_json::from_value::<MetaFields>(row.get(1))?,
                ))
            })
            .collect::<anyhow::Result<HashMap<_, _>>>()?;
        federations
            .into_iter()
            .map(|federation| {
                let meta = metas.remove(&federation.federation_id);
                let name = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<String>("federation_name"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_name")
                            .cloned()
                    });
                let expiry_timestamp = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<u64>("federation_expiry_timestamp"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_expiry_timestamp")
                            .and_then(|value| value.parse().ok())
                    });
                let successor = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<String>("federation_successor"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_successor")
                            .cloned()
                    });
                (federation, name, expiry_timestamp, successor)
            })
            .map(|(federation, name, expiry_timestamp, successor)| {
                let federation_id = federation.federation_id;
                let (first_peer_id, first_peer_url) = federation
                    .config
                    .global
                    .api_endpoints
                    .first_key_value()
                    .context("Federation config has no guardians")?;
                let invite = InviteCode::new(
                    first_peer_url.url.clone(),
                    *first_peer_id,
                    federation_id,
                    None,
                )
                .to_string();

                Ok(FederationSummary {
                    id: federation_id,
                    name,
                    last_7d_activity: activity
                        .remove(&federation_id)
                        .unwrap_or_else(|| vec![NO_ACTIVITY; 7]),
                    deposits: self.combined_assets(&federation, &assets, &snapshots)?,
                    invite,
                    nostr_votes: ratings.remove(&federation_id).unwrap_or(FederationRating {
                        count: 0,
                        avg: None,
                    }),
                    health: federation_health
                        .get(&federation_id)
                        .copied()
                        .unwrap_or(FederationHealth::Offline),
                    expiry_timestamp,
                    // A successor that is not an invite code, or names this
                    // federation itself, points nowhere users can move to
                    successor_id: successor
                        .as_deref()
                        .and_then(|successor| successor.trim().parse::<InviteCode>().ok())
                        .map(|invite| invite.federation_id())
                        .filter(|successor_id| *successor_id != federation_id),
                    successor,
                })
            })
            .collect()
    }

    /// Each federation's transactions and volume on each of the last `days`
    /// days, oldest first
    async fn federations_activity(
        &self,
        days: u32,
    ) -> anyhow::Result<HashMap<FederationId, Vec<FederationActivity>>> {
        #[derive(Debug, FromRow)]
        struct ActivityRow {
            federation_id: Vec<u8>,
            day: NaiveDate,
            tx_count: i64,
            volume_msat: i64,
        }

        let today = chrono::Utc::now().date_naive();
        let rows = query::<ActivityRow>(
            &self.connection().await?,
            "SELECT federation_id, day, tx_count, volume_msat
             FROM federation_daily_activity
             WHERE day > $1",
            &[&(today - chrono::Duration::days(days.into()))],
        )
        .await?;

        let mut by_federation = HashMap::<FederationId, HashMap<NaiveDate, ActivityRow>>::new();
        for row in rows {
            by_federation
                .entry(federation_id_from_db(&row.federation_id)?)
                .or_default()
                .insert(row.day, row);
        }
        Ok(by_federation
            .into_iter()
            .map(|(federation_id, days_activity)| {
                let activity = last_n_day_iter(today, days)
                    .map(|day| {
                        days_activity
                            .get(&day)
                            .map_or(NO_ACTIVITY, |row| FederationActivity {
                                num_transactions: row.tx_count as u64,
                                amount_transferred: Amount::from_msats(row.volume_msat as u64),
                            })
                    })
                    .collect();
                (federation_id, activity)
            })
            .collect())
    }

    /// What each federation's wallet holds: all peg-ins minus all peg-outs
    async fn federation_assets(&self) -> anyhow::Result<HashMap<FederationId, Amount>> {
        self.connection()
            .await?
            .query(
                "SELECT federation_id, wallet_in_msat - wallet_out_msat FROM federation_totals",
                &[],
            )
            .await?
            .iter()
            .map(|row| {
                Ok((
                    federation_id_from_db(&row.get::<_, Vec<u8>>(0))?,
                    Amount::from_msats(
                        u64::try_from(row.get::<_, i64>(1)).context("Negative wallet balance")?,
                    ),
                ))
            })
            .collect()
    }

    pub async fn get_federation_assets(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Option<Amount>> {
        let federation = self.observed_federation(federation_id).await?;
        let services = Services::new(self.clone());
        let total_assets_msat = query_value::<i64>(
            &self.connection().await?,
            "SELECT COALESCE(
                 (SELECT wallet_in_msat - wallet_out_msat FROM federation_totals WHERE federation_id = $1),
                 0
             )",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        let mut amount = Amount::from_msats(
            u64::try_from(total_assets_msat).context("Negative wallet balance")?,
        );
        for module in self
            .modules()
            .modules()
            .filter(|module| module.reports_assets())
        {
            let kind = module.kind().to_string();
            if crate::util::module_instance_id(&federation.config, &kind).is_some() {
                let Some((value, observed_at)) = services
                    .observation(federation_id, &format!("assets:{kind}"))
                    .await?
                else {
                    return Ok(None);
                };
                if chrono::Utc::now().signed_duration_since(observed_at)
                    > chrono::Duration::minutes(5)
                {
                    return Ok(None);
                }
                amount = amount
                    .checked_add(Amount::from_msats(
                        value.as_u64().context("Invalid asset snapshot")?,
                    ))
                    .context("Federation asset total overflow")?;
            }
        }
        Ok(Some(amount))
    }

    async fn asset_snapshots(&self) -> anyhow::Result<HashMap<(FederationId, String), Amount>> {
        self.connection().await?.query(
            "SELECT federation_id, kind, data FROM federation_observations WHERE kind LIKE 'assets:%' AND observed_at >= now() - INTERVAL '5 minutes'", &[]
        ).await?.into_iter().map(|row| {
            let value: serde_json::Value = row.get(2);
            Ok(((federation_id_from_db(&row.get::<_, Vec<u8>>(0))?, row.get(1)),
                Amount::from_msats(value.as_u64().context("Invalid asset snapshot")?)))
        }).collect()
    }

    fn combined_assets(
        &self,
        federation: &Federation,
        legacy: &HashMap<FederationId, Amount>,
        snapshots: &HashMap<(FederationId, String), Amount>,
    ) -> anyhow::Result<Option<Amount>> {
        let mut total = legacy
            .get(&federation.federation_id)
            .copied()
            .unwrap_or(Amount::ZERO);
        for module in self
            .modules()
            .modules()
            .filter(|module| module.reports_assets())
        {
            let kind = module.kind().to_string();
            if crate::util::module_instance_id(&federation.config, &kind).is_some() {
                let Some(amount) =
                    snapshots.get(&(federation.federation_id, format!("assets:{kind}")))
                else {
                    return Ok(None);
                };
                total = total
                    .checked_add(*amount)
                    .context("Federation asset total overflow")?;
            }
        }
        Ok(Some(total))
    }

    pub async fn totals(&self) -> anyhow::Result<FedimintTotals> {
        #[derive(Debug, FromRow)]
        struct FedimintTotalsResult {
            federations: i64,
            tx_count: i64,
            tx_volume: i64,
        }

        let offline_federations = self
            .get_guardian_health_summary()
            .await?
            .values()
            .filter(|&health| *health == FederationHealth::Offline)
            .count() as u64;

        // Reads the counters maintained by the triggers introduced in v9 instead of
        // aggregating over `transactions` and `transaction_inputs`, which meant a full
        // scan of both tables (~1.3GB) on every request.
        let totals = query_one::<FedimintTotalsResult>(
            &self.connection().await?,
            // language=postgresql
            "
                SELECT (SELECT count(*) from federations)::bigint as federations,
                       COALESCE(sum(tx_count), 0)::bigint         as tx_count,
                       COALESCE(sum(tx_volume_msat), 0)::bigint   as tx_volume
                FROM federation_totals
            ",
            &[],
        )
        .await?;

        Ok(FedimintTotals {
            federations: (totals.federations as u64) - offline_federations,
            tx_count: totals.tx_count as u64,
            tx_volume: Amount::from_msats(totals.tx_volume as u64),
        })
    }
}

fn last_n_day_iter(now: NaiveDate, days: u32) -> impl Iterator<Item = NaiveDate> {
    (0..days)
        .rev()
        .map(move |day| now - chrono::Duration::days(day as i64))
}

#[cfg(test)]
mod tests {
    use super::last_n_day_iter;

    #[test]
    fn test_day_iter() {
        let now = chrono::offset::Utc::now().date_naive();
        let days = 7;
        let last_7_days = last_n_day_iter(now, days).collect::<Vec<_>>();
        assert_eq!(last_7_days.len(), days as usize);
        assert_eq!(last_7_days[6], now);
        assert_eq!(last_7_days[0], now - chrono::Duration::days(6));
    }
}
