use std::collections::BTreeMap;

use anyhow::Context;
use fedimint_api_client::api::DynGlobalApi;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::net::api_announcement::SignedApiAnnouncement;
use fedimint_core::secp256k1::{PublicKey, SECP256K1};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use futures::future::join_all;
use tracing::{debug, warn};

use crate::federation::observer::FederationObserver;

impl FederationObserver {
    pub(super) async fn api_for_federation(
        &self,
        federation_id: FederationId,
        config: &ClientConfig,
    ) -> anyhow::Result<DynGlobalApi> {
        let peers = self.resolved_api_urls(federation_id, config).await?;
        DynGlobalApi::new(self.connectors().clone(), peers, None)
    }

    pub(super) async fn resolved_api_urls(
        &self,
        federation_id: FederationId,
        config: &ClientConfig,
    ) -> anyhow::Result<BTreeMap<PeerId, SafeUrl>> {
        let mut urls = config
            .global
            .api_endpoints
            .iter()
            .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone()))
            .collect::<BTreeMap<_, _>>();

        let Some(public_keys) = &config.global.broadcast_public_keys else {
            return Ok(urls);
        };

        for (peer_id, announcement) in self.load_api_announcements(federation_id).await? {
            let Some(public_key) = public_keys.get(&peer_id) else {
                warn!(%federation_id, %peer_id, "Ignoring API announcement for unknown guardian");
                continue;
            };
            if !announcement.verify(SECP256K1, public_key) {
                warn!(%federation_id, %peer_id, "Ignoring API announcement with invalid signature");
                continue;
            }
            if let Some(url) = urls.get_mut(&peer_id) {
                *url = announcement.api_announcement.api_url;
            }
        }

        Ok(urls)
    }

    pub(super) async fn refresh_api_announcements(
        &self,
        federation_id: FederationId,
        config: &ClientConfig,
        api: &DynGlobalApi,
    ) -> anyhow::Result<bool> {
        let Some(public_keys) = &config.global.broadcast_public_keys else {
            return Ok(false);
        };

        let responses = join_all(
            config
                .global
                .api_endpoints
                .keys()
                .map(|&peer_id| async move { (peer_id, api.api_announcements(peer_id).await) }),
        )
        .await
        .into_iter()
        .filter_map(|(peer_id, result)| match result {
            Ok(announcements) => Some(announcements),
            Err(error) => {
                debug!(%federation_id, %peer_id, %error, "Failed to fetch guardian API announcements");
                None
            }
        })
        .collect::<Vec<_>>();

        let mut current = self.load_api_announcements(federation_id).await?;
        current.retain(|peer_id, announcement| {
            public_keys
                .get(peer_id)
                .is_some_and(|key| announcement.verify(SECP256K1, key))
        });
        let updates = select_new_announcements(public_keys, &current, &responses);
        if updates.is_empty() {
            return Ok(false);
        }

        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        for (peer_id, announcement) in &updates {
            let encoded = announcement.consensus_encode_to_vec();
            let nonce = announcement.api_announcement.nonce.to_be_bytes();
            transaction
                .execute(
                    "INSERT INTO guardian_api_announcements
                         (federation_id, guardian_id, nonce, announcement)
                     VALUES ($1, $2, $3, $4)
                     ON CONFLICT (federation_id, guardian_id) DO UPDATE
                         SET nonce = EXCLUDED.nonce,
                             announcement = EXCLUDED.announcement
                         WHERE guardian_api_announcements.nonce < EXCLUDED.nonce",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &(peer_id.to_usize() as i32),
                        &&nonce[..],
                        &encoded,
                    ],
                )
                .await?;
            debug!(%federation_id, %peer_id, url = %announcement.api_announcement.api_url, "Stored newer guardian API announcement");
        }
        transaction.commit().await?;

        Ok(true)
    }

    async fn load_api_announcements(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<BTreeMap<PeerId, SignedApiAnnouncement>> {
        let rows = self
            .connection()
            .await?
            .query(
                "SELECT guardian_id, nonce, announcement
                 FROM guardian_api_announcements
                 WHERE federation_id = $1",
                &[&federation_id.consensus_encode_to_vec()],
            )
            .await?;

        rows.into_iter()
            .map(|row| {
                let guardian_id = row.get::<_, i32>("guardian_id");
                let peer_id = u16::try_from(guardian_id)
                    .map(PeerId::from)
                    .context("Guardian ID outside PeerId range")?;
                let nonce = row.get::<_, Vec<u8>>("nonce");
                let announcement = SignedApiAnnouncement::consensus_decode_whole(
                    &row.get::<_, Vec<u8>>("announcement"),
                    &Default::default(),
                )
                .context("Invalid signed API announcement in database")?;
                if nonce.as_slice() != announcement.api_announcement.nonce.to_be_bytes() {
                    anyhow::bail!("API announcement nonce does not match stored index");
                }
                Ok((peer_id, announcement))
            })
            .collect()
    }
}

fn select_new_announcements(
    public_keys: &BTreeMap<PeerId, PublicKey>,
    current: &BTreeMap<PeerId, SignedApiAnnouncement>,
    responses: &[BTreeMap<PeerId, SignedApiAnnouncement>],
) -> BTreeMap<PeerId, SignedApiAnnouncement> {
    let mut newest = current.clone();

    for response in responses {
        let valid = response.iter().all(|(peer_id, announcement)| {
            public_keys
                .get(peer_id)
                .is_some_and(|key| announcement.verify(SECP256K1, key))
        });
        if !valid {
            continue;
        }

        for (&peer_id, announcement) in response {
            let replace = newest.get(&peer_id).is_none_or(|known| {
                known.api_announcement.nonce < announcement.api_announcement.nonce
            });
            if replace {
                newest.insert(peer_id, announcement.clone());
            }
        }
    }

    newest.retain(|peer_id, announcement| {
        current
            .get(peer_id)
            .is_none_or(|known| known.api_announcement.nonce < announcement.api_announcement.nonce)
    });
    newest
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use fedimint_core::net::api_announcement::ApiAnnouncement;
    use fedimint_core::secp256k1::{Keypair, SecretKey};

    use super::*;

    fn signed(url: &str, nonce: u64, key: &Keypair) -> SignedApiAnnouncement {
        ApiAnnouncement::new(SafeUrl::from_str(url).expect("valid URL"), nonce).sign(SECP256K1, key)
    }

    #[test]
    fn selects_only_newer_verified_announcements() {
        let peer = PeerId::from(1);
        let key = Keypair::from_secret_key(
            SECP256K1,
            &SecretKey::from_slice(&[1; 32]).expect("valid secret key"),
        );
        let other_key = Keypair::from_secret_key(
            SECP256K1,
            &SecretKey::from_slice(&[2; 32]).expect("valid secret key"),
        );
        let public_keys = BTreeMap::from([(peer, PublicKey::from_keypair(&key))]);
        let current = BTreeMap::from([(peer, signed("wss://old.example", 1, &key))]);

        let older = BTreeMap::from([(peer, signed("wss://older.example", 0, &key))]);
        let forged = BTreeMap::from([(peer, signed("wss://forged.example", 3, &other_key))]);
        let newer = BTreeMap::from([(peer, signed("wss://new.example", 2, &key))]);

        let selected =
            select_new_announcements(&public_keys, &current, &[older, forged, newer.clone()]);
        assert_eq!(selected, newer);
    }

    #[test]
    fn rejects_entire_response_when_one_signature_is_invalid() {
        let peer0 = PeerId::from(0);
        let peer1 = PeerId::from(1);
        let key0 = Keypair::from_secret_key(
            SECP256K1,
            &SecretKey::from_slice(&[1; 32]).expect("valid secret key"),
        );
        let key1 = Keypair::from_secret_key(
            SECP256K1,
            &SecretKey::from_slice(&[2; 32]).expect("valid secret key"),
        );
        let public_keys = BTreeMap::from([
            (peer0, PublicKey::from_keypair(&key0)),
            (peer1, PublicKey::from_keypair(&key1)),
        ]);
        let mixed = BTreeMap::from([
            (peer0, signed("wss://valid.example", 1, &key0)),
            (peer1, signed("wss://forged.example", 1, &key0)),
        ]);

        assert!(select_new_announcements(&public_keys, &BTreeMap::new(), &[mixed]).is_empty());
    }

    #[test]
    fn big_endian_nonce_bytes_preserve_u64_ordering() {
        // PostgreSQL compares the fixed-width BYTEA nonce in the upsert. This
        // must keep working above i64::MAX, where a BIGINT column cannot hold
        // the announcement's u64 nonce.
        for (older, newer) in [(0_u64, 1_u64), (255, 256), (i64::MAX as u64, u64::MAX)] {
            assert!(older.to_be_bytes() < newer.to_be_bytes());
        }
    }
}
