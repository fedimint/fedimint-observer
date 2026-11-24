// Pin public HTTPS gateway addresses and disable redirects.
use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, ensure, Context};
use fedimint_core::config::FederationId;
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::gateway_api::RoutingInfo;

use crate::probes::ProbeError;

pub(crate) fn classify_send_error(error: reqwest::Error) -> ProbeError {
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

pub(crate) use fmo_core::public_address::public_ip;

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
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(20))
            .resolve_to_addrs(host, &addresses)
            .build()?;
        Ok(Self {
            url: url.join("/routing_info")?,
            client,
        })
    }
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
