//! Minimal LND REST client covering what probing needs.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::{RequestBuilder, Response};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::{DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use serde::{Deserialize, Deserializer};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct LndClient {
    client: reqwest::Client,
    base_url: String,
    macaroon_hex: String,
}

#[derive(Debug, Deserialize)]
pub struct GetInfo {
    pub identity_pubkey: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub synced_to_graph: bool,
}

#[derive(Debug, Deserialize)]
struct ListChannels {
    #[serde(default)]
    channels: Vec<Channel>,
}

#[derive(Debug, Deserialize)]
struct Channel {
    #[serde(default)]
    active: bool,
    #[serde(default, deserialize_with = "de_u64")]
    local_balance: u64,
    #[serde(default)]
    local_constraints: Option<ChannelConstraints>,
}

#[derive(Debug, Deserialize)]
struct ChannelConstraints {
    #[serde(default, deserialize_with = "de_u64")]
    chan_reserve_sat: u64,
}

#[derive(Debug, Deserialize)]
struct QueryRoutesResponse {
    #[serde(default)]
    routes: Vec<serde_json::Value>,
}

/// Route as returned by QueryRoutes. Kept as raw JSON so it can be passed
/// back to SendToRouteV2 without loss.
#[derive(Debug, Clone)]
pub struct Route {
    pub raw: serde_json::Value,
    pub hops: u32,
    pub total_fees_msat: u64,
}

/// Result of a single HTLC attempt (`lnrpc.HTLCAttempt`).
#[derive(Debug, Clone, Deserialize)]
pub struct HtlcAttempt {
    #[serde(default)]
    pub status: String,
    #[serde(default, deserialize_with = "de_u64")]
    pub attempt_time_ns: u64,
    #[serde(default, deserialize_with = "de_u64")]
    pub resolve_time_ns: u64,
    #[serde(default)]
    pub failure: Option<HtlcFailure>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HtlcFailure {
    #[serde(default)]
    pub code: String,
    /// Proto3 omits zero values, so a missing index means our own node
    #[serde(default)]
    pub failure_source_index: u32,
}

/// Error returned by LND's REST gateway
#[derive(Debug, Deserialize)]
struct LndError {
    #[serde(default)]
    message: String,
}

#[derive(Debug)]
pub enum QueryRoutesError {
    NoRoute(String),
    Other(anyhow::Error),
}

/// LND encodes 64 bit integers as strings in JSON
fn de_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrNum {
        Str(String),
        Num(u64),
    }

    match Option::<StrOrNum>::deserialize(deserializer)? {
        None => Ok(0),
        Some(StrOrNum::Num(n)) => Ok(n),
        Some(StrOrNum::Str(s)) if s.is_empty() => Ok(0),
        Some(StrOrNum::Str(s)) => s.parse().map_err(serde::de::Error::custom),
    }
}

impl LndClient {
    pub fn new(
        base_url: &str,
        macaroon_path: &Path,
        tls_cert_path: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let macaroon = std::fs::read(macaroon_path)
            .with_context(|| format!("Reading macaroon {}", macaroon_path.display()))?;

        let mut builder = reqwest::Client::builder();
        if let Some(tls_cert_path) = tls_cert_path {
            let cert = CertificateDer::from_pem_file(tls_cert_path)
                .with_context(|| format!("Reading TLS cert {}", tls_cert_path.display()))?;
            builder = builder.use_preconfigured_tls(pinned_tls_config(cert));
        }

        Ok(LndClient {
            client: builder.build()?,
            base_url: base_url.trim_end_matches('/').to_owned(),
            macaroon_hex: hex::encode(macaroon),
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        self.client
            .request(method, format!("{}{}", self.base_url, path))
            .header("Grpc-Metadata-macaroon", &self.macaroon_hex)
    }

    async fn error_message(response: Response) -> String {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<LndError>(&body)
            .map(|err| err.message)
            .unwrap_or(body);
        format!("LND returned {status}: {message}")
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let response = self.request(reqwest::Method::GET, path).send().await?;
        if !response.status().is_success() {
            bail!(Self::error_message(response).await);
        }
        Ok(response.json().await?)
    }

    pub async fn get_info(&self) -> anyhow::Result<GetInfo> {
        self.get_json("/v1/getinfo").await
    }

    /// Largest amount that can be sent through a single one of our channels.
    /// Probes are single path, so anything above this would fail locally and
    /// must not be attributed to the gateway.
    pub async fn max_spendable_msat(&self) -> anyhow::Result<u64> {
        let channels: ListChannels = self.get_json("/v1/channels?active_only=true").await?;
        Ok(channels
            .channels
            .iter()
            .filter(|channel| channel.active)
            .map(|channel| {
                let reserve = channel
                    .local_constraints
                    .as_ref()
                    .map_or(0, |c| c.chan_reserve_sat);
                channel.local_balance.saturating_sub(reserve) * 1000
            })
            .max()
            .unwrap_or(0))
    }

    pub async fn query_routes(
        &self,
        node_pub_key: &str,
        amount_sat: u64,
        fee_limit_sat: u64,
    ) -> Result<Route, QueryRoutesError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/v1/graph/routes/{node_pub_key}/{amount_sat}"),
            )
            .query(&[
                ("fee_limit.fixed", fee_limit_sat.to_string()),
                ("use_mission_control", "true".to_owned()),
            ])
            .send()
            .await
            .map_err(|e| QueryRoutesError::Other(e.into()))?;

        if !response.status().is_success() {
            let message = Self::error_message(response).await;
            return Err(if is_no_route_error(&message) {
                QueryRoutesError::NoRoute(message)
            } else {
                QueryRoutesError::Other(anyhow::anyhow!(message))
            });
        }

        let routes: QueryRoutesResponse = response
            .json()
            .await
            .map_err(|e| QueryRoutesError::Other(e.into()))?;
        let raw = routes
            .routes
            .into_iter()
            .next()
            .ok_or_else(|| QueryRoutesError::NoRoute("QueryRoutes returned no route".into()))?;
        Route::from_raw(raw).map_err(QueryRoutesError::Other)
    }

    /// Sends an HTLC along `route` with the given payment hash and waits for
    /// it to resolve.
    pub async fn send_to_route(
        &self,
        payment_hash: [u8; 32],
        route: &Route,
    ) -> anyhow::Result<HtlcAttempt> {
        let response = self
            .request(reqwest::Method::POST, "/v2/router/route/send")
            .json(&json!({
                "payment_hash": BASE64.encode(payment_hash),
                "route": route.raw,
                // Return temporary failures instead of retrying them so we
                // learn where the probe failed
                "skip_temp_err": true,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            bail!(Self::error_message(response).await);
        }
        Ok(response.json().await?)
    }
}

fn is_no_route_error(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("unable to find a path") || message.contains("no route")
}

impl Route {
    fn from_raw(raw: serde_json::Value) -> anyhow::Result<Self> {
        let hops = raw
            .get("hops")
            .and_then(|hops| hops.as_array())
            .context("Route without hops")?
            .len();
        anyhow::ensure!(hops > 0, "Route without hops");
        let total_fees_msat = raw
            .get("total_fees_msat")
            .and_then(|fee| match fee {
                serde_json::Value::String(s) => s.parse().ok(),
                other => other.as_u64(),
            })
            .unwrap_or(0);
        Ok(Route {
            raw,
            hops: hops as u32,
            total_fees_msat,
        })
    }

    /// Adds an MPP record with a random payment address to the final hop, as a
    /// real payment would. Without it nodes that require payment secrets
    /// could reject the probe for that reason alone.
    pub fn with_payment_addr(mut self, payment_addr: [u8; 32], amount_msat: u64) -> Self {
        if let Some(last_hop) = self
            .raw
            .get_mut("hops")
            .and_then(|hops| hops.as_array_mut())
            .and_then(|hops| hops.last_mut())
            .and_then(|hop| hop.as_object_mut())
        {
            last_hop.insert(
                "mpp_record".to_owned(),
                json!({
                    "payment_addr": BASE64.encode(payment_addr),
                    "total_amt_msat": amount_msat.to_string(),
                }),
            );
        }
        self
    }
}

/// LND's auto-generated certificate is self-signed and marked as CA, which
/// webpki refuses as a server certificate. Instead of disabling verification
/// (which would leak the macaroon to anyone in the middle) we pin the exact
/// certificate and still check the handshake signatures against it.
fn pinned_tls_config(cert: CertificateDer<'static>) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedCertVerifier { cert, provider }))
        .with_no_client_auth()
}

#[derive(Debug)]
struct PinnedCertVerifier {
    cert: CertificateDer<'static>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.cert.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "LND presented a certificate different from the pinned one".to_owned(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_htlc_attempt() {
        let attempt: HtlcAttempt = serde_json::from_str(
            r#"{
                "attempt_id": "1",
                "status": "FAILED",
                "attempt_time_ns": "1700000000000000000",
                "resolve_time_ns": "1700000001250000000",
                "failure": {
                    "code": "INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS",
                    "failure_source_index": 3
                },
                "preimage": null
            }"#,
        )
        .unwrap();
        assert_eq!(attempt.status, "FAILED");
        assert_eq!(
            attempt.resolve_time_ns - attempt.attempt_time_ns,
            1_250_000_000
        );
        let failure = attempt.failure.unwrap();
        assert_eq!(failure.code, "INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS");
        assert_eq!(failure.failure_source_index, 3);
    }

    #[test]
    fn missing_failure_source_index_is_local() {
        let attempt: HtlcAttempt = serde_json::from_str(
            r#"{"status": "FAILED", "failure": {"code": "TEMPORARY_CHANNEL_FAILURE"}}"#,
        )
        .unwrap();
        assert_eq!(attempt.failure.unwrap().failure_source_index, 0);
    }

    #[test]
    fn route_from_raw_and_payment_addr() {
        let raw = serde_json::json!({
            "total_fees_msat": "2010",
            "hops": [{"pub_key": "a"}, {"pub_key": "b", "mpp_record": null}]
        });
        let route = Route::from_raw(raw)
            .unwrap()
            .with_payment_addr([7; 32], 10_000_000);
        assert_eq!(route.hops, 2);
        assert_eq!(route.total_fees_msat, 2010);
        assert_eq!(
            route.raw["hops"][1]["mpp_record"]["total_amt_msat"],
            "10000000"
        );
        assert!(route.raw["hops"][0].get("mpp_record").is_none());
    }

    #[test]
    fn detects_no_route_errors() {
        assert!(is_no_route_error(
            "LND returned 500: unable to find a path to destination"
        ));
        assert!(!is_no_route_error("LND returned 500: permission denied"));
    }

    #[test]
    fn parses_channels() {
        let channels: ListChannels = serde_json::from_str(
            r#"{"channels": [
                {"active": true, "local_balance": "500000", "local_constraints": {"chan_reserve_sat": "5000"}},
                {"active": false, "local_balance": "9000000"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(channels.channels[0].local_balance, 500_000);
        assert!(!channels.channels[1].active);
    }
}
