use std::collections::BTreeMap;

use fedimint_core::config::{FederationId, GlobalClientConfig};
use fedimint_core::net::api_announcement::SignedApiAnnouncement;
use fedimint_core::secp256k1::{self, SECP256K1};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use tracing::{debug, info, warn};

#[cfg(test)]
mod tests;

/// Observation kind string used when persisting guardian endpoints to
/// `federation_observations`.
pub const GUARDIAN_ENDPOINTS_OBSERVATION_KIND: &str = "guardian_endpoints";

/// Supported guardian endpoint URL schemes.
const ALLOWED_URL_SCHEMES: &[&str] = &["ws", "wss", "http", "https", "iroh"];

/// Validates whether the given URL uses a supported communication scheme for
/// Fedimint guardians.
pub fn is_valid_endpoint_scheme(url: &SafeUrl) -> bool {
    let scheme = url.scheme();
    ALLOWED_URL_SCHEMES.contains(&scheme)
}

/// A cryptographically verified guardian API route.
///
/// Invariant: instances of `VerifiedRoute` represent routes whose Schnorr
/// signatures have been successfully verified against the guardian's broadcast
/// public key and whose URL scheme is permitted for Fedimint communications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRoute {
    announcement: SignedApiAnnouncement,
}

impl VerifiedRoute {
    /// Attempts to verify a `SignedApiAnnouncement` against the guardian's
    /// broadcast public key and checks the URL scheme.
    pub fn verify(
        announcement: SignedApiAnnouncement,
        guardian_pub_key: &secp256k1::PublicKey,
    ) -> Result<Self, &'static str> {
        if !announcement.verify(SECP256K1, guardian_pub_key) {
            return Err("Invalid cryptographic signature");
        }
        if !is_valid_endpoint_scheme(&announcement.api_announcement.api_url) {
            return Err("Unsupported URL scheme");
        }
        Ok(Self { announcement })
    }

    pub fn url(&self) -> &SafeUrl {
        &self.announcement.api_announcement.api_url
    }

    pub fn nonce(&self) -> u64 {
        self.announcement.api_announcement.nonce
    }

    pub fn announcement(&self) -> &SignedApiAnnouncement {
        &self.announcement
    }
}

/// Map of PeerId to verified signed API route.
pub type VerifiedRoutesMap = BTreeMap<PeerId, VerifiedRoute>;
pub type GuardianEndpointsMap = VerifiedRoutesMap;

/// Resolves the active API peers for a federation by overlaying verified
/// announced endpoints onto the static config endpoints.
///
/// Falls back to the config endpoint for any guardian without an accepted
/// announcement.
pub fn active_api_peers(
    global: &GlobalClientConfig,
    announced: Option<&VerifiedRoutesMap>,
) -> BTreeMap<PeerId, SafeUrl> {
    global
        .api_endpoints
        .iter()
        .map(|(&peer_id, endpoint)| {
            let url = announced
                .and_then(|map| map.get(&peer_id))
                .map(|route| route.url().clone())
                .unwrap_or_else(|| endpoint.url.clone());
            (peer_id, url)
        })
        .collect()
}

/// Verifies incoming signed endpoint announcements against guardian broadcast
/// public keys and merges valid ones whose nonce is strictly higher than
/// existing records.
///
/// Returns the updated map and a boolean flag indicating if any endpoints
/// changed.
pub fn verify_and_merge_announcements(
    federation_id: FederationId,
    broadcast_public_keys: Option<&BTreeMap<PeerId, secp256k1::PublicKey>>,
    existing: &VerifiedRoutesMap,
    incoming: &BTreeMap<PeerId, SignedApiAnnouncement>,
) -> (VerifiedRoutesMap, bool) {
    let Some(keys) = broadcast_public_keys else {
        warn!(
            %federation_id,
            "Federation has no broadcast public keys in config; rejecting all dynamic endpoint announcements"
        );
        return (existing.clone(), false);
    };

    let mut updated = existing.clone();
    let mut changed = false;

    for (&peer_id, announcement) in incoming {
        // 1. Verify peer belongs to the federation broadcast public keys
        let Some(guardian_pub_key) = keys.get(&peer_id) else {
            warn!(
                %federation_id,
                %peer_id,
                "Ignoring announcement for peer not present in broadcast public keys"
            );
            continue;
        };

        // 2. Cryptographically verify signature and URL scheme into a VerifiedRoute
        let verified_route = match VerifiedRoute::verify(announcement.clone(), guardian_pub_key) {
            Ok(route) => route,
            Err(reason) => {
                warn!(
                    %federation_id,
                    %peer_id,
                    reason,
                    "Rejecting invalid dynamic endpoint announcement"
                );
                continue;
            }
        };

        // 3. Accept announcement only if nonce is strictly higher than currently
        //    accepted
        let new_nonce = verified_route.nonce();
        if let Some(current) = updated.get(&peer_id) {
            if new_nonce <= current.nonce() {
                debug!(
                    %federation_id,
                    %peer_id,
                    new_nonce,
                    current_nonce = current.nonce(),
                    "Ignoring announcement with stale or equal nonce"
                );
                continue;
            }
        }

        let old_url = updated.get(&peer_id).map(|route| route.url().to_string());
        info!(
            %federation_id,
            %peer_id,
            old_url = old_url.as_deref().unwrap_or("<config>"),
            new_url = %verified_route.url(),
            new_nonce,
            "Accepted verified guardian endpoint announcement"
        );
        updated.insert(peer_id, verified_route);
        changed = true;
    }

    (updated, changed)
}

/// Verifies each answering peer's response before choosing the highest nonce.
/// This prevents a forged high-nonce announcement in one response from
/// suppressing a valid announcement returned by another peer.
pub fn verify_and_merge_announcement_sets<'a>(
    federation_id: FederationId,
    broadcast_public_keys: Option<&BTreeMap<PeerId, secp256k1::PublicKey>>,
    existing: &VerifiedRoutesMap,
    incoming_sets: impl IntoIterator<Item = &'a BTreeMap<PeerId, SignedApiAnnouncement>>,
) -> (VerifiedRoutesMap, bool) {
    let mut updated = existing.clone();
    let mut changed = false;

    for incoming in incoming_sets {
        let (next, response_changed) = verify_and_merge_announcements(
            federation_id,
            broadcast_public_keys,
            &updated,
            incoming,
        );
        updated = next;
        changed |= response_changed;
    }

    (updated, changed)
}

/// Re-verifies stored endpoint announcements and applies the same monotonic
/// nonce rule before admitting them into the active routing cache.
pub fn verify_and_merge_restored_announcements(
    federation_id: FederationId,
    broadcast_public_keys: Option<&BTreeMap<PeerId, secp256k1::PublicKey>>,
    existing: &VerifiedRoutesMap,
    stored: &BTreeMap<PeerId, SignedApiAnnouncement>,
) -> (VerifiedRoutesMap, bool) {
    verify_and_merge_announcements(federation_id, broadcast_public_keys, existing, stored)
}
