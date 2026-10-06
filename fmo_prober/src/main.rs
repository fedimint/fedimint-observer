//! Lightning prober for Fedimint Observer.
//!
//! Periodically probes the LN nodes of all gateways known to Observer from a
//! funded LND node and pushes the measurements to Observer. Runs independently
//! from Observer and only talks to it over its HTTP API.

mod lnd;
mod observer;
mod probe;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use fmo_api_types::{GatewayProbeResult, GatewayProbeSubmission};
use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::lnd::LndClient;
use crate::observer::ObserverClient;
use crate::probe::{probe_ladder, ProbeConfig};

/// Submit results in batches of this size while a round is still running
const SUBMIT_BATCH_SIZE: usize = 100;
const SUBMIT_ATTEMPTS: u32 = 5;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Base URL of the Observer API, e.g. `https://observer.fedimint.org/api`
    #[arg(long, env = "FMP_OBSERVER_URL")]
    observer_url: String,

    /// Bearer token matching the Observer's `FO_PROBER_AUTH`
    #[arg(long, env = "FMP_OBSERVER_AUTH")]
    observer_auth: String,

    /// Identifies this prober (e.g. its location) in Observer
    #[arg(long, env = "FMP_PROBER_ID")]
    prober_id: String,

    /// LND REST endpoint, e.g. `https://localhost:8080`
    #[arg(long, env = "FMP_LND_REST_URL")]
    lnd_rest_url: String,

    /// Path to an LND macaroon allowing `offchain:read`, `offchain:write` and
    /// `info:read`
    #[arg(long, env = "FMP_LND_MACAROON")]
    lnd_macaroon: PathBuf,

    /// Path to LND's `tls.cert`. If set the certificate is pinned, otherwise
    /// the system/webpki roots are used.
    #[arg(long, env = "FMP_LND_TLS_CERT")]
    lnd_tls_cert: Option<PathBuf>,

    /// Time between the start of two probing rounds
    #[arg(long, env = "FMP_PROBE_INTERVAL_SECS", default_value_t = 1800)]
    probe_interval_secs: u64,

    /// Probe amounts in sats, probed in ascending order per gateway until one
    /// fails
    #[arg(
        long,
        env = "FMP_PROBE_AMOUNTS_SAT",
        value_delimiter = ',',
        default_value = "10000,100000,1000000"
    )]
    probe_amounts_sat: Vec<u64>,

    /// Maximum routing fee for probe routes. Probes never settle so no fees
    /// are paid, but routes must be realistic for a payment.
    #[arg(long, env = "FMP_MAX_FEE_SAT", default_value_t = 1000)]
    max_fee_sat: u64,

    /// How long to wait for a probe HTLC to resolve
    #[arg(long, env = "FMP_PROBE_TIMEOUT_SECS", default_value_t = 60)]
    probe_timeout_secs: u64,

    /// Only probe these LN nodes (comma separated public keys). Probes all
    /// gateways known to Observer if unset.
    #[arg(long, env = "FMP_NODE_ALLOWLIST", value_delimiter = ',')]
    node_allowlist: Vec<String>,
}

struct Prober {
    args: Args,
    lnd: LndClient,
    observer: ObserverClient,
    own_pub_key: String,
    allowlist: HashSet<String>,
    probe_config: ProbeConfig,
}

impl Prober {
    async fn run_round(&self) -> anyhow::Result<()> {
        let targets: Vec<String> = self
            .observer
            .probe_targets()
            .await?
            .into_iter()
            .map(|key| key.to_lowercase())
            .filter(|key| *key != self.own_pub_key)
            .filter(|key| self.allowlist.is_empty() || self.allowlist.contains(key))
            .collect();

        // Probes never settle, so our balance only changes through other
        // activity on the node. Checking once per round is enough.
        let max_spendable_msat = self.lnd.max_spendable_msat().await?;
        info!(
            targets = targets.len(),
            max_spendable_msat, "Starting probing round"
        );

        let mut pending = Vec::new();
        let mut submitted = 0;
        // Sequential on purpose: concurrent in-flight probes would lock up our
        // own outbound liquidity and cause failures unrelated to the gateways.
        for node_pub_key in &targets {
            let results = probe_ladder(
                &self.lnd,
                self.probe_config,
                node_pub_key,
                &self.args.probe_amounts_sat,
                max_spendable_msat,
            )
            .await;
            for result in &results {
                info!(
                    node = %node_pub_key,
                    amount_msat = result.amount_msat,
                    outcome = result.outcome.as_str(),
                    latency_ms = ?result.latency_ms,
                    failure_code = ?result.failure_code,
                    "Probe finished"
                );
            }
            pending.extend(results);

            if pending.len() >= SUBMIT_BATCH_SIZE {
                submitted += self.submit(std::mem::take(&mut pending)).await;
            }
        }
        submitted += self.submit(pending).await;

        info!(submitted, "Finished probing round");
        Ok(())
    }

    /// Submits results, retrying with backoff. Submissions are idempotent on
    /// the Observer side so retrying after a lost response is safe.
    async fn submit(&self, results: Vec<GatewayProbeResult>) -> u64 {
        if results.is_empty() {
            return 0;
        }

        let submission = GatewayProbeSubmission {
            prober_id: self.args.prober_id.clone(),
            results,
        };
        let mut delay = Duration::from_secs(2);
        for attempt in 1..=SUBMIT_ATTEMPTS {
            match self.observer.submit(&submission).await {
                Ok(stored) => return stored,
                Err(e) => {
                    warn!(attempt, "Submitting probe results failed: {e:#}");
                    if attempt < SUBMIT_ATTEMPTS {
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                    }
                }
            }
        }
        error!(
            dropped = submission.results.len(),
            "Giving up submitting probe results"
        );
        0
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive("info".parse().unwrap())
                .from_env()
                .unwrap(),
        )
        .init();

    anyhow::ensure!(
        !args.probe_amounts_sat.is_empty() && !args.probe_amounts_sat.contains(&0),
        "Probe amounts must be non-empty and positive"
    );
    args.probe_amounts_sat.sort_unstable();
    args.probe_amounts_sat.dedup();

    let lnd = LndClient::new(
        &args.lnd_rest_url,
        &args.lnd_macaroon,
        args.lnd_tls_cert.as_deref(),
    )?;
    let info = lnd.get_info().await?;
    info!(
        alias = %info.alias,
        pubkey = %info.identity_pubkey,
        synced_to_graph = info.synced_to_graph,
        "Connected to LND"
    );
    if !info.synced_to_graph {
        warn!("LND is not synced to the graph yet, expect spurious no_route results");
    }

    let prober = Prober {
        observer: ObserverClient::new(&args.observer_url, &args.observer_auth)?,
        lnd,
        own_pub_key: info.identity_pubkey.to_lowercase(),
        allowlist: args
            .node_allowlist
            .iter()
            .map(|key| key.trim().to_lowercase())
            .filter(|key| !key.is_empty())
            .collect(),
        probe_config: ProbeConfig {
            max_fee_sat: args.max_fee_sat,
            timeout: Duration::from_secs(args.probe_timeout_secs),
        },
        args,
    };

    let mut interval = tokio::time::interval(Duration::from_secs(prober.args.probe_interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(e) = prober.run_round().await {
            error!("Probing round failed: {e:#}");
        }
    }
}
