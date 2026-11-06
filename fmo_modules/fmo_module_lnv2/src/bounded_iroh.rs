//! Short-lived probe transports: no URL-keyed pool survives a probe, including
//! failed targets. Guardian/session transports retain their upstream limits.
use std::time::Duration;

use anyhow::{anyhow, ensure, Context};
use fedimint_core::config::FederationId;
use fedimint_core::module::{IrohGatewayRequest, IrohGatewayResponse, FEDIMINT_GATEWAY_ALPN};
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::gateway_api::RoutingInfo;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};

use crate::probes::ProbeError;

pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Also returns the request latency. It excludes binding the local endpoints,
/// as HTTPS latency excludes DNS; it includes address discovery and the QUIC
/// handshake, as HTTPS latency includes the TCP and TLS handshakes.
pub(crate) async fn routing_info(
    url: &SafeUrl,
    fed: &FederationId,
) -> Result<(Option<RoutingInfo>, Duration), ProbeError> {
    // An invalid identity or a local bind failure says nothing about the gateway.
    let host = url
        .host_str()
        .context("Missing Iroh identity")
        .map_err(ProbeError::not_probed)?;
    let next_only = url.path() == "/v1";
    let stable_id: iroh::NodeId = host.parse().map_err(ProbeError::not_probed)?;
    let next_id: iroh_next::EndpointId = host.parse().map_err(ProbeError::not_probed)?;
    let (stable, next) = tokio::time::timeout(Duration::from_secs(5), async {
        let stable = if next_only {
            None
        } else {
            Some(
                iroh::Endpoint::builder()
                    .relay_mode(iroh::RelayMode::Disabled)
                    .add_discovery(|_| Some(iroh::discovery::pkarr::PkarrResolver::n0_dns()))
                    .add_discovery(|_| Some(iroh::discovery::dns::DnsDiscovery::n0_dns()))
                    .bind()
                    .await?,
            )
        };
        let next = iroh_next::Endpoint::builder(iroh_next::endpoint::presets::Minimal)
            .relay_mode(iroh_next::RelayMode::Default)
            .address_lookup(iroh_next::address_lookup::PkarrResolver::n0_dns())
            .address_lookup(iroh_next::address_lookup::DnsAddressLookup::n0_dns())
            .bind()
            .await?;
        anyhow::Ok((stable, next))
    })
    .await
    .context("Binding local Iroh endpoints timed out")
    .and_then(|bound| bound)
    .map_err(ProbeError::not_probed)?;
    let request = serde_json::to_vec(&IrohGatewayRequest {
        route: "/routing_info".to_owned(),
        params: Some(serde_json::to_value(fed).map_err(ProbeError::not_probed)?),
        password: None,
    })
    .map_err(ProbeError::not_probed)?;
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let mut attempts = FuturesUnordered::new();
        attempts.push(request_next(&next, next_id.into(), &request).boxed());
        if let Some(stable) = &stable {
            attempts.push(request_stable(stable, stable_id.into(), &request).boxed());
        }
        // The first answer wins; if both fail, report the attempt that got
        // furthest, so one transport's answer is not hidden by the other's
        // failure.
        let mut failure: Option<ProbeError> = None;
        while let Some(attempt) = attempts.next().await {
            match attempt {
                Ok(info) => return Ok(info),
                Err(error) => {
                    failure = Some(match failure {
                        Some(previous) => previous.furthest(error),
                        None => error,
                    });
                }
            }
        }
        Err(failure.unwrap_or_else(|| ProbeError::not_probed(anyhow!("No Iroh transport"))))
    })
    .await;
    let elapsed = started.elapsed();
    // Close on success, invalid/oversized responses, timeout and failed dials.
    // Cleanup remains inside the probe concurrency budget.
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            async {
                if let Some(stable) = &stable {
                    stable.close().await;
                }
            },
            next.close()
        );
    })
    .await;
    result
        .map_err(|_| ProbeError::unreachable(anyhow!("Gateway probe timed out")))?
        .map(|info| (info, elapsed))
}

pub(crate) async fn request_stable(
    endpoint: &iroh::Endpoint,
    target: iroh::NodeAddr,
    request: &[u8],
) -> Result<Option<RoutingInfo>, ProbeError> {
    let connection = endpoint
        .connect(target, FEDIMINT_GATEWAY_ALPN)
        .await
        .map_err(ProbeError::unreachable)?;
    // Connected: from here on the gateway is reachable.
    let result = async {
        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(request).await?;
        send.finish()?;
        anyhow::Ok(recv.read_to_end(MAX_RESPONSE_BYTES).await?)
    }
    .await;
    connection.close(0u32.into(), b"probe completed");
    decode(&result.map_err(ProbeError::bad_response)?).map_err(ProbeError::bad_response)
}

fn decode(bytes: &[u8]) -> anyhow::Result<Option<RoutingInfo>> {
    let response: IrohGatewayResponse = serde_json::from_slice(bytes)?;
    ensure!(
        response.status == 200,
        "Gateway returned status {}",
        response.status
    );
    Ok(serde_json::from_value(response.body)?)
}

pub(crate) async fn request_next(
    endpoint: &iroh_next::Endpoint,
    target: iroh_next::EndpointAddr,
    request: &[u8],
) -> Result<Option<RoutingInfo>, ProbeError> {
    let connection = endpoint
        .connect(target, FEDIMINT_GATEWAY_ALPN)
        .await
        .map_err(ProbeError::unreachable)?;
    // Connected: from here on the gateway is reachable.
    let result = async {
        let (mut send, mut recv) = connection.open_bi().await?;
        send.write_all(request).await?;
        send.finish()?;
        anyhow::Ok(recv.read_to_end(MAX_RESPONSE_BYTES).await?)
    }
    .await;
    connection.close(0u32.into(), b"probe completed");
    decode(&result.map_err(ProbeError::bad_response)?).map_err(ProbeError::bad_response)
}
