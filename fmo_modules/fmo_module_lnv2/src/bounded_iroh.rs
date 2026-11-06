//! Short-lived probe transports: no URL-keyed pool survives a probe, including
//! failed targets. Guardian/session transports retain their upstream limits.
use std::time::Duration;

use anyhow::{ensure, Context};
use fedimint_core::config::FederationId;
use fedimint_core::module::{IrohGatewayRequest, IrohGatewayResponse, FEDIMINT_GATEWAY_ALPN};
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::gateway_api::RoutingInfo;
use futures::FutureExt;

pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Also returns the request latency. It excludes binding the local endpoints,
/// as HTTPS latency excludes DNS; it includes address discovery and the QUIC
/// handshake, as HTTPS latency includes the TCP and TLS handshakes.
pub(crate) async fn routing_info(
    url: &SafeUrl,
    fed: &FederationId,
) -> anyhow::Result<(Option<RoutingInfo>, Duration)> {
    let host = url.host_str().context("Missing Iroh identity")?;
    let next_only = url.path() == "/v1";
    let stable_id: iroh::NodeId = host.parse()?;
    let next_id: iroh_next::EndpointId = host.parse()?;
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
    .await??;
    let request = serde_json::to_vec(&IrohGatewayRequest {
        route: "/routing_info".to_owned(),
        params: Some(serde_json::to_value(fed)?),
        password: None,
    })?;
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let next_request = request_next(&next, next_id.into(), &request).boxed();
        if let Some(stable) = &stable {
            let stable_request = request_stable(stable, stable_id.into(), &request).boxed();
            futures::future::select_ok(vec![stable_request, next_request])
                .await
                .map(|(response, _)| response)
        } else {
            next_request.await
        }
    })
    .await;
    let elapsed = started.elapsed();
    // Close on success, invalid/oversized responses, timeout and failed dials.
    // Cleanup remains inside the external-I/O concurrency budget.
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
        .context("Gateway probe timed out")?
        .map(|info| (info, elapsed))
}

pub(crate) async fn request_stable(
    endpoint: &iroh::Endpoint,
    target: iroh::NodeAddr,
    request: &[u8],
) -> anyhow::Result<Option<RoutingInfo>> {
    let connection = endpoint.connect(target, FEDIMINT_GATEWAY_ALPN).await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(request).await?;
    send.finish()?;
    let result = recv.read_to_end(MAX_RESPONSE_BYTES).await;
    connection.close(0u32.into(), b"probe completed");
    decode(&result?)
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
) -> anyhow::Result<Option<RoutingInfo>> {
    let connection = endpoint.connect(target, FEDIMINT_GATEWAY_ALPN).await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(request).await?;
    send.finish()?;
    let result = recv.read_to_end(MAX_RESPONSE_BYTES).await;
    connection.close(0u32.into(), b"probe completed");
    decode(&result?)
}
