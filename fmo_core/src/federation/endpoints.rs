use std::collections::BTreeMap;

use fedimint_core::config::{FederationId, GlobalClientConfig};
use fedimint_core::net::api_announcement::SignedApiAnnouncement;
use fedimint_core::secp256k1::{self, SECP256K1};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use tracing::{debug, info, warn};

#[cfg(test)]
mod tests;

/// `federation_observations` kind of stored guardian routes
pub const GUARDIAN_ENDPOINTS_OBSERVATION_KIND: &str = "guardian_endpoints";

/// Schemes the guardian connector can dial
const ALLOWED_URL_SCHEMES: &[&str] = &["ws", "wss", "iroh"];

/// Whether the guardian connector can dial `url`
fn is_valid_endpoint_scheme(url: &SafeUrl) -> bool {
    let scheme = url.scheme();
    ALLOWED_URL_SCHEMES.contains(&scheme)
}

/// Whether `url` names a non-public IP or `localhost`; hostnames aren't
/// resolved
fn non_public_host(url: &SafeUrl) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| !crate::public_address::public_ip(ip))
}

/// A guardian route whose signature and scheme have been verified
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRoute {
    announcement: SignedApiAnnouncement,
}

impl VerifiedRoute {
    /// Checks the signature against the guardian's key, and the scheme
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

/// Verified signed API routes by guardian
pub type GuardianEndpointsMap = BTreeMap<PeerId, VerifiedRoute>;

/// Config endpoints with each accepted announcement applied
pub fn active_api_peers(
    global: &GlobalClientConfig,
    announced: Option<&GuardianEndpointsMap>,
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

/// Adds verified announcements with a higher nonce; returns whether any did
pub fn verify_and_merge_announcements(
    federation_id: FederationId,
    global: &GlobalClientConfig,
    existing: &GuardianEndpointsMap,
    incoming: &BTreeMap<PeerId, SignedApiAnnouncement>,
) -> (GuardianEndpointsMap, bool) {
    let Some(keys) = global.broadcast_public_keys.as_ref() else {
        warn!(
            %federation_id,
            "Federation has no broadcast public keys in config; rejecting all dynamic endpoint announcements"
        );
        return (existing.clone(), false);
    };

    let mut updated = existing.clone();
    let mut changed = false;

    for (&peer_id, announcement) in incoming {
        // Federation members only
        let Some(guardian_pub_key) = keys.get(&peer_id) else {
            warn!(
                %federation_id,
                %peer_id,
                "Ignoring announcement for peer not present in broadcast public keys"
            );
            continue;
        };

        // Signature and scheme
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

        // A public guardian can't redirect the observer into private networks.
        if non_public_host(verified_route.url())
            && !global
                .api_endpoints
                .get(&peer_id)
                .is_some_and(|endpoint| non_public_host(&endpoint.url))
        {
            warn!(
                %federation_id,
                %peer_id,
                "Rejecting announcement that moves a public guardian to a non-public address"
            );
            continue;
        }

        // Only a strictly higher nonce
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
