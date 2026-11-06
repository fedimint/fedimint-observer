//! Pin public HTTPS gateway addresses and disable redirects. A guardian's
//! registry must not turn the observer into a client for private HTTP services.
use std::net::{IpAddr, SocketAddr};

use anyhow::{ensure, Context};
use fedimint_core::config::FederationId;
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::gateway_api::RoutingInfo;

pub(crate) fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0)
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 198 && b == 51 && c == 100)
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| public_ip(IpAddr::V4(ip)))
            .unwrap_or_else(|| {
                let segments = ip.segments();
                !ip.is_loopback()
                    && !ip.is_unspecified()
                    && !ip.is_multicast()
                    && !ip.is_unique_local()
                    && !ip.is_unicast_link_local()
                    && !(segments[0] == 0x2001 && segments[1] == 0xdb8)
                    && (segments[0] & 0xe000) == 0x2000
            }),
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
            .timeout(std::time::Duration::from_secs(20))
            .resolve_to_addrs(host, &addresses)
            .build()?;
        // The official gateway connector joins the same absolute route.
        Ok(Self {
            url: url.join("/routing_info")?,
            client,
        })
    }
    pub(crate) async fn routing_info(
        &self,
        fed: &FederationId,
    ) -> anyhow::Result<Option<RoutingInfo>> {
        let mut response = self
            .client
            .post(self.url.clone())
            .json(fed)
            .send()
            .await?
            .error_for_status()?;
        const MAX_BYTES: usize = 1024 * 1024;
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= MAX_BYTES as u64),
            "Gateway routing response is too large"
        );
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                body.len() + chunk.len() <= MAX_BYTES,
                "Gateway routing response is too large"
            );
            body.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }
}
