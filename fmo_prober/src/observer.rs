//! Client for the Fedimint Observer probe API.

use anyhow::bail;
use fmo_api_types::GatewayProbeSubmission;

#[derive(Debug, Clone)]
pub struct ObserverClient {
    client: reqwest::Client,
    base_url: String,
    auth: String,
}

impl ObserverClient {
    pub fn new(base_url: &str, auth: &str) -> anyhow::Result<Self> {
        Ok(ObserverClient {
            client: reqwest::Client::builder().build()?,
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth: auth.to_owned(),
        })
    }

    async fn check(response: reqwest::Response) -> anyhow::Result<reqwest::Response> {
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Observer returned {status}: {body}");
        }
        Ok(response)
    }

    /// LN node public keys of all gateways currently registered with any
    /// observed federation.
    pub async fn probe_targets(&self) -> anyhow::Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/gateways/probe-targets", self.base_url))
            .send()
            .await?;
        Ok(Self::check(response).await?.json().await?)
    }

    /// Submits probe results, returns the number of newly stored results.
    pub async fn submit(&self, submission: &GatewayProbeSubmission) -> anyhow::Result<u64> {
        let response = self
            .client
            .post(format!("{}/gateways/probes", self.base_url))
            .bearer_auth(&self.auth)
            .json(submission)
            .send()
            .await?;
        Ok(Self::check(response).await?.json().await?)
    }
}
