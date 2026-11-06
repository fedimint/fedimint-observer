//! Pin public HTTPS gateway addresses and disable redirects. A guardian's
//! registry must not turn the observer into a client for private HTTP services.
use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, ensure, Context};
use fedimint_core::config::FederationId;
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::gateway_api::RoutingInfo;

use crate::probes::ProbeError;

pub(crate) fn classify_send_error(error: reqwest::Error) -> ProbeError {
    // reqwest does not mark an EOF before HTTP headers as a connect error.
    // Hyper reports it as an incomplete message, which still means that the
    // gateway gave no response to this probe.
    let mut cause = std::error::Error::source(&error);
    while let Some(source) = cause {
        let incomplete = source
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_incomplete_message);
        let disconnected = source.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            )
        });
        if incomplete || disconnected {
            return ProbeError::unreachable(error);
        }
        cause = source.source();
    }
    if error.is_connect() || error.is_timeout() {
        ProbeError::unreachable(error)
    } else {
        ProbeError::bad_response(error)
    }
}

/// IPv4 blocks a probe must not reach: the IANA IPv4 Special-Purpose Address
/// Registry entries (RFC 6890 and updates), multicast and reserved space.
/// Blocks with a few globally reachable anycast addresses are excluded whole.
const NON_PUBLIC_V4: &[(u32, u32)] = &[
    (0x0000_0000, 8),  // 0.0.0.0/8 this network
    (0x0A00_0000, 8),  // 10.0.0.0/8 private
    (0x6440_0000, 10), // 100.64.0.0/10 shared address space
    (0x7F00_0000, 8),  // 127.0.0.0/8 loopback
    (0xA9FE_0000, 16), // 169.254.0.0/16 link local
    (0xAC10_0000, 12), // 172.16.0.0/12 private
    (0xC000_0000, 24), // 192.0.0.0/24 IETF protocol assignments
    (0xC000_0200, 24), // 192.0.2.0/24 documentation
    (0xC058_6300, 24), // 192.88.99.0/24 deprecated 6to4 relay anycast
    (0xC0A8_0000, 16), // 192.168.0.0/16 private
    (0xC612_0000, 15), // 198.18.0.0/15 benchmarking
    (0xC633_6400, 24), // 198.51.100.0/24 documentation
    (0xCB00_7100, 24), // 203.0.113.0/24 documentation
    (0xE000_0000, 4),  // 224.0.0.0/4 multicast
    (0xF000_0000, 4),  // 240.0.0.0/4 reserved, including broadcast
];

/// Within global unicast 2000::/3, the IANA IPv6 Special-Purpose Address
/// Registry blocks that are not public hosts, or that embed or tunnel IPv4.
/// Everything outside 2000::/3 (loopback, mapped, NAT64, discard, ULA, link
/// local, multicast, SRv6 SIDs, ...) is rejected before this table.
const NON_PUBLIC_V6: &[(u128, u32)] = &[
    (0x2001_0000_u128 << 96, 23), /* 2001::/23 IETF protocol assignments: Teredo, benchmarking,
                                   * ORCHIDv2 */
    (0x2001_0db8_u128 << 96, 32), // 2001:db8::/32 documentation
    (0x2002_u128 << 112, 16),     // 2002::/16 6to4
    (0x3fff_0000_u128 << 96, 20), // 3fff::/20 documentation
];

fn in_prefix<T>(address: T, network: T, length: u32, bits: u32) -> bool
where
    T: Copy + PartialEq + std::ops::Shr<u32, Output = T>,
{
    length == 0 || address >> (bits - length) == network >> (bits - length)
}

pub(crate) fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let ip = u32::from(ip);
            !NON_PUBLIC_V4
                .iter()
                .any(|&(network, length)| in_prefix(ip, network, length, 32))
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            // ::ffff:a.b.c.d reaches the embedded IPv4 address directly.
            Some(ip) => public_ip(IpAddr::V4(ip)),
            None => {
                let ip = u128::from(ip);
                in_prefix(ip, 0x2000_u128 << 112, 3, 128)
                    && !NON_PUBLIC_V6
                        .iter()
                        .any(|&(network, length)| in_prefix(ip, network, length, 128))
            }
        },
    }
}

pub(crate) struct Target {
    url: reqwest::Url,
    client: reqwest::Client,
}

impl Target {
    pub(crate) async fn prepare(url: &SafeUrl) -> anyhow::Result<Self> {
        ensure!(
            url.scheme() == "https",
            "Only public HTTPS or Iroh gateway probes are supported"
        );
        let url = url.clone().to_unsafe();
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "Gateway URLs with credentials are not probed"
        );
        let host = url.host_str().context("Gateway URL has no host")?;
        let port = url
            .port_or_known_default()
            .context("Gateway URL has no port")?;
        let addresses = if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::lookup_host((host, port)),
            )
            .await??
            .collect::<Vec<_>>()
        };
        ensure!(
            !addresses.is_empty() && addresses.iter().all(|address| public_ip(address.ip())),
            "Gateway does not resolve exclusively to public addresses"
        );
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            // An address that drops packets fails here, not after the full
            // request timeout, so it holds a probe slot for seconds.
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(20))
            .resolve_to_addrs(host, &addresses)
            .build()?;
        // The official gateway connector joins the same absolute route.
        Ok(Self {
            url: url.join("/routing_info")?,
            client,
        })
    }
    /// Connection failures and timeouts before a response are `Unreachable`;
    /// anything after the gateway answered is a `BadResponse`.
    pub(crate) async fn routing_info(
        &self,
        fed: &FederationId,
    ) -> Result<Option<RoutingInfo>, ProbeError> {
        let mut response = self
            .client
            .post(self.url.clone())
            .json(fed)
            .send()
            .await
            .map_err(classify_send_error)?
            .error_for_status()
            .map_err(ProbeError::bad_response)?;
        const MAX_BYTES: usize = super::bounded_iroh::MAX_RESPONSE_BYTES;
        let too_large =
            || ProbeError::bad_response(anyhow!("Gateway routing response is too large"));
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(ProbeError::bad_response)? {
            if body.len() + chunk.len() > MAX_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(ProbeError::bad_response)
    }
}
