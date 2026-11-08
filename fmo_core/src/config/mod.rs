use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, ensure};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use fedimint_api_client::api::FederationApiExt;
use fedimint_api_client::query::FilterMap;
use fedimint_connectors::error::ServerError;
use fedimint_core::config::{
    ClientConfig, FederationId, GlobalClientConfig, JsonClientConfig, PeerUrl,
};
use fedimint_core::endpoint_constants::CLIENT_CONFIG_ENDPOINT;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::module::{ApiRequestErased, CoreConsensusVersion};
use fedimint_core::util::backoff_util::custom_backoff;
use fedimint_core::PeerId;
use tracing::{debug, warn};

use crate::config::id::fetch_federation_id;
use crate::config::meta::fetch_federation_meta;
use crate::config::modules::fetch_federation_module_kinds;
use crate::error::{ClientError, Result};
use crate::federation::observer::FederationObserver;
use crate::util::{config_to_json, make_room};
use crate::AppState;

/// Helper API that exposes the federation id
pub mod id;
/// Helper API that unifies config meta and override meta, applying lenient
/// parsing
pub mod meta;

/// Helper API that exposes the federation modules
pub mod modules;
/// CORS for these routes comes from the permissive layer on the whole app.
/// `/:invite/gateways` is the Lightning module's.
pub fn get_config_routes() -> Router<AppState> {
    Router::new()
        .route("/:invite", get(fetch_federation_config))
        .route("/:invite/meta", get(fetch_federation_meta))
        .route("/:invite/id", get(fetch_federation_id))
        .route("/:invite/module_kinds", get(fetch_federation_module_kinds))
}

pub async fn fetch_federation_config(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> Result<Json<JsonClientConfig>> {
    Ok(state
        .federation_config_cache
        .fetch_config_cached(&invite, &state.federation_observer)
        .await?
        .into())
}

type DownloadSlots = Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;

#[derive(Default, Debug, Clone)]
pub struct FederationConfigCache {
    federations: Arc<tokio::sync::RwLock<HashMap<FederationId, JsonClientConfig>>>,
    /// Invites whose download failed, when and how. Failures are not cached
    /// like configs, but for [`FAILED_INVITE_RETRY_DELAY`] the invite fails
    /// again without contacting anyone, so clients asking for it in a loop
    /// don't make the observer contact its servers in one.
    failures: Arc<std::sync::Mutex<HashMap<String, (Instant, ClientError)>>>,
    /// One download per invite at a time: concurrent requests for the same
    /// invite wait for the one in progress and use its result
    downloads: DownloadSlots,
}

impl FederationConfigCache {
    pub async fn fetch_config_cached(
        &self,
        invite: &InviteCode,
        observer: &FederationObserver,
    ) -> anyhow::Result<JsonClientConfig> {
        let federation_id = invite.federation_id();
        if let Some(config) = self.cached(federation_id).await {
            return Ok(config);
        }

        let key = invite.to_string();
        let slot = DownloadSlot::take(&self.downloads, &key);
        let _download = slot.lock().await;
        // The download this one waited for may have stored a result
        if let Some(config) = self.cached(federation_id).await {
            return Ok(config);
        }
        if let Some(error) = self.recent_failure(&key) {
            return Err(error.into());
        }

        let config = match fetch_config_inner(invite, observer).await {
            Ok(config) => config,
            Err(error) => {
                if let Some(client_error) = error.downcast_ref::<ClientError>() {
                    self.record_failure(key.clone(), client_error.clone());
                }
                return Err(error);
            }
        };
        let mut cache = self.federations.write().await;
        if !cache.contains_key(&federation_id) {
            make_room(&mut cache, MAX_CACHED_CONFIGS);
        }
        if let Some(replaced) = cache.insert(federation_id, config.clone()) {
            if replaced != config {
                warn!("Config for federation {federation_id} changed");
            }
        }

        Ok(config)
    }

    async fn cached(&self, federation_id: FederationId) -> Option<JsonClientConfig> {
        self.federations.read().await.get(&federation_id).cloned()
    }

    /// How the last download of the invite failed, if it did within
    /// [`FAILED_INVITE_RETRY_DELAY`]
    fn recent_failure(&self, key: &str) -> Option<ClientError> {
        let mut failures = self.failures.lock().expect("failure cache lock poisoned");
        match failures.get(key) {
            Some((at, error)) if at.elapsed() < FAILED_INVITE_RETRY_DELAY => Some(error.clone()),
            Some(_) => {
                failures.remove(key);
                None
            }
            None => None,
        }
    }

    fn record_failure(&self, key: String, error: ClientError) {
        let mut failures = self.failures.lock().expect("failure cache lock poisoned");
        failures.retain(|_, (at, _)| at.elapsed() < FAILED_INVITE_RETRY_DELAY);
        if !failures.contains_key(&key) {
            make_room(&mut failures, MAX_CACHED_CONFIGS);
        }
        failures.insert(key, (Instant::now(), error));
    }
}

/// A request's claim on the download of an invite; the last request done with
/// it removes it
struct DownloadSlot<'a> {
    slots: &'a DownloadSlots,
    key: &'a str,
    slot: Arc<tokio::sync::Mutex<()>>,
}

impl<'a> DownloadSlot<'a> {
    fn take(slots: &'a DownloadSlots, key: &'a str) -> Self {
        let slot = slots
            .lock()
            .expect("download slots lock poisoned")
            .entry(key.to_owned())
            .or_default()
            .clone();
        Self { slots, key, slot }
    }
}

impl std::ops::Deref for DownloadSlot<'_> {
    type Target = tokio::sync::Mutex<()>;

    fn deref(&self) -> &Self::Target {
        &self.slot
    }
}

impl Drop for DownloadSlot<'_> {
    fn drop(&mut self) {
        let mut slots = self.slots.lock().expect("download slots lock poisoned");
        // The map's reference and ours: no other request waits for it
        if Arc::strong_count(&self.slot) <= 2 {
            slots.remove(self.key);
        }
    }
}

/// Federations anyone can ask about through an invite code
const MAX_CACHED_CONFIGS: usize = 256;
/// Invite codes name the servers to contact, so they can name unreachable
/// ones
pub const INVITE_FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an invite whose download failed fails without a new attempt
const FAILED_INVITE_RETRY_DELAY: Duration = Duration::from_secs(30);

async fn fetch_config_inner(
    invite: &InviteCode,
    observer: &FederationObserver,
) -> anyhow::Result<JsonClientConfig> {
    let raw_config = download_client_config(invite, observer).await?;
    config_to_json(raw_config, observer.modules())
}

/// Downloads the config behind an invite code from the federation's guardians.
///
/// Uses the observer's shared guardian clients: every client built per
/// request (`fedimint_api_client::download_from_invite_code` builds two) keeps
/// its guardian connections open for the life of the process. A few failed
/// attempts are retried, within [`INVITE_FETCH_TIMEOUT`]. Fails with a
/// [`ClientError`]: 504 if the guardians didn't answer in time, 502 if they
/// couldn't be reached or answered wrongly.
pub(crate) async fn download_client_config(
    invite: &InviteCode,
    observer: &FederationObserver,
) -> anyhow::Result<ClientConfig> {
    let download = fedimint_core::util::retry(
        "Downloading federation config",
        custom_backoff(Duration::from_millis(500), Duration::from_secs(5), Some(3)),
        || try_download_client_config(invite, observer),
    );
    download_outcome(
        tokio::time::timeout(INVITE_FETCH_TIMEOUT, download)
            .await
            .ok(),
    )
}

/// The status a config download fails with; `None` if it timed out
fn download_outcome<T>(result: Option<anyhow::Result<T>>) -> anyhow::Result<T> {
    match result {
        Some(Ok(value)) => Ok(value),
        // E.g. too many guardian clients: keeps its own status
        Some(Err(error)) if error.downcast_ref::<ClientError>().is_some() => Err(error),
        Some(Err(error)) => {
            debug!(?error, "Federation config download failed");
            Err(error.context(ClientError::bad_gateway(
                "Could not download the federation config from its guardians",
            )))
        }
        None => Err(ClientError::gateway_timeout(
            "Timed out downloading the federation config from its guardians",
        )
        .into()),
    }
}

/// One attempt, the way `fedimint_api_client::try_download_client_config`
/// does it: the invite's guardians name all guardians, a threshold of which
/// must then agree on the config
async fn try_download_client_config(
    invite: &InviteCode,
    observer: &FederationObserver,
) -> anyhow::Result<ClientConfig> {
    let federation_id = invite.federation_id();
    let invite_api = observer.lookup_api(
        &endpoints_config(
            invite
                .peers()
                .into_iter()
                .map(|(peer_id, url)| {
                    (
                        peer_id,
                        PeerUrl {
                            url,
                            name: String::new(),
                        },
                    )
                })
                .collect(),
        ),
        invite.api_secret(),
    )?;
    let api_endpoints = invite_api
        .request_with_strategy(
            FilterMap::new(move |config: ClientConfig| {
                if config.global.calculate_federation_id() != federation_id {
                    return Err(ServerError::ConditionFailed(anyhow!(
                        "Federation id in invite code does not match client config"
                    )));
                }
                Ok(config.global.api_endpoints)
            }),
            CLIENT_CONFIG_ENDPOINT.to_owned(),
            ApiRequestErased::default(),
        )
        .await?;

    let config = observer
        .lookup_api(&endpoints_config(api_endpoints), invite.api_secret())?
        .request_current_consensus::<ClientConfig>(
            CLIENT_CONFIG_ENDPOINT.to_owned(),
            ApiRequestErased::default(),
        )
        .await?;
    ensure!(
        config.calculate_federation_id() == federation_id,
        "Guardians returned the config of another federation"
    );
    Ok(config)
}

/// A config naming only guardian endpoints, which is all
/// [`FederationObserver::federation_api`] looks at
fn endpoints_config(api_endpoints: BTreeMap<PeerId, PeerUrl>) -> GlobalClientConfig {
    GlobalClientConfig {
        api_endpoints,
        broadcast_public_keys: None,
        consensus_version: CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests;
