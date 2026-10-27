use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use axum::Json;
use fedimint_core::config::{FederationId, JsonClientConfig};
use fedimint_core::invite_code::InviteCode;
use fmo_api_types::FederationSuccessor;
use tracing::debug;
use tracing::log::warn;

use crate::config::meta::{parse_meta_lenient, MetaFields};
use crate::util::merge_metas;
use crate::AppState;

/// Shutdown dates the operator sets, `FO_FEDERATION_EXPIRY`, for federations
/// whose guardians can't announce one, such as those without a meta module:
/// comma-separated `<federation id>=<unix seconds>`. They take precedence
/// over what the guardians announce.
#[derive(Debug, Clone, Default)]
pub struct FederationExpiries(Arc<BTreeMap<FederationId, u64>>);

impl FromStr for FederationExpiries {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        s.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let (federation_id, time) = entry
                    .split_once('=')
                    .with_context(|| format!("{entry:?} is not <federation id>=<unix seconds>"))?;
                Ok((
                    FederationId::from_str(federation_id.trim())
                        .with_context(|| format!("{federation_id:?} is not a federation id"))?,
                    time.trim()
                        .parse()
                        .with_context(|| format!("{time:?} is not a number of unix seconds"))?,
                ))
            })
            .collect::<anyhow::Result<_>>()
            .map(|expiries| Self(Arc::new(expiries)))
    }
}

impl FederationExpiries {
    /// The meta fields the operator sets for the federation
    pub fn meta(&self, federation_id: FederationId) -> MetaFields {
        self.0
            .get(&federation_id)
            .map(|time| ("federation_expiry_timestamp".to_owned(), (*time).into()))
            .into_iter()
            .collect()
    }
}

/// The meta fields in a federation's config
pub fn config_meta(meta: &BTreeMap<String, String>) -> MetaFields {
    parse_meta_lenient(
        meta.iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned().into())),
    )
}

/// When the federation shuts down, in unix seconds: a base 10 number, in a
/// string like every meta field
pub fn expiry_timestamp(meta: &MetaFields) -> Option<u64> {
    match meta.get("federation_expiry_timestamp")? {
        serde_json::Value::Number(time) => time.as_u64(),
        serde_json::Value::String(time) => time.trim().parse().ok(),
        _ => None,
    }
}

/// The federation users should move to, named by an invite code
pub fn successor(meta: &MetaFields) -> Option<FederationSuccessor> {
    let invite = InviteCode::from_str(meta.get("federation_successor")?.as_str()?.trim()).ok()?;
    Some(FederationSuccessor {
        id: invite.federation_id(),
        invite: invite.to_string(),
    })
}

pub async fn federation_meta(
    cfg: &JsonClientConfig,
    state: &AppState,
) -> crate::error::Result<Json<MetaFields>> {
    let federation_id = cfg.global.calculate_federation_id();
    let maybe_consensus_meta = state.consensus_meta_cache.fetch_meta_cached(cfg).await;

    let meta_fields_config = config_meta(&cfg.global.meta);

    let maybe_meta_override = if let Some(override_url) = meta_fields_config
        .get("meta_override_url")
        .or_else(|| meta_fields_config.get("meta_external_url")) // Fedi legacy field
        .and_then(|url| url.as_str().map(ToOwned::to_owned))
    {
        debug!("fetching {override_url}");
        // Without the override, the consensus and config meta still apply
        state
            .meta_override_cache
            .fetch_meta_cached(&override_url, federation_id)
            .await
            .inspect_err(|e| warn!("Failed to fetch meta fields from {override_url}: {e:?}"))
            .ok()
    } else {
        None
    };

    Ok(Json(merge_metas(&[
        state.federation_observer.expiries().meta(federation_id),
        maybe_consensus_meta.unwrap_or_default(),
        maybe_meta_override.unwrap_or_default(),
        meta_fields_config,
    ])))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use fedimint_core::config::FederationId;
    use fedimint_core::invite_code::InviteCode;
    use fedimint_core::util::SafeUrl;
    use fedimint_core::PeerId;

    use super::{expiry_timestamp, successor, FederationExpiries};
    use crate::config::meta::{parse_meta_lenient, MetaFields};

    const FEDERATION_ID: &str = "654fa91b928eb4b3ab061683ed1e08caf4125468e89300947ea94945cd26ce98";

    // As federations set them: strings, parsed as JSON where they are JSON
    fn meta(fields: &[(&str, &str)]) -> MetaFields {
        parse_meta_lenient(
            fields
                .iter()
                .map(|(key, value)| (key.to_string(), (*value).into())),
        )
    }

    #[test]
    fn the_expiry_is_a_number_of_unix_seconds() {
        let expiry = |value| expiry_timestamp(&meta(&[("federation_expiry_timestamp", value)]));
        assert_eq!(expiry("1739149200"), Some(1_739_149_200));
        assert_eq!(expiry("\"1739149200\""), Some(1_739_149_200));
        assert_eq!(expiry("soon"), None);
        assert_eq!(expiry("-1"), None);
        assert_eq!(expiry_timestamp(&meta(&[])), None);
    }

    #[test]
    fn the_successor_is_an_invite_code() {
        let federation_id = FederationId::from_str(FEDERATION_ID).unwrap();
        let invite = InviteCode::new(
            SafeUrl::parse("wss://fedimint.example.com").unwrap(),
            PeerId::from(0),
            federation_id,
            None,
        )
        .to_string();
        let found = successor(&meta(&[("federation_successor", &invite)])).unwrap();
        assert_eq!(found.id, federation_id);
        assert_eq!(found.invite, invite);
        assert!(successor(&meta(&[("federation_successor", "fed11nope")])).is_none());
    }

    #[test]
    fn operators_set_expiries_by_federation_id() {
        let federation_id = FederationId::from_str(FEDERATION_ID).unwrap();
        let expiries = FederationExpiries::from_str(&format!(" {FEDERATION_ID} = 1767225600 ,"))
            .expect("valid");
        assert_eq!(
            expiry_timestamp(&expiries.meta(federation_id)),
            Some(1_767_225_600)
        );
        assert!(FederationExpiries::from_str("")
            .unwrap()
            .meta(federation_id)
            .is_empty());
        assert!(FederationExpiries::from_str("654f=1767225600").is_err());
        assert!(FederationExpiries::from_str(FEDERATION_ID).is_err());
        assert!(FederationExpiries::from_str(&format!("{FEDERATION_ID}=2027-01-01")).is_err());
    }
}
