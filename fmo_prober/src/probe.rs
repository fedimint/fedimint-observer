//! Probing logic: sending HTLCs with unknown payment hashes and classifying
//! where and why they failed.

use std::time::{Duration, Instant};

use chrono::Utc;
use fmo_api_types::{GatewayProbeResult, ProbeOutcome};
use tracing::{debug, warn};

use crate::lnd::{HtlcAttempt, LndClient, QueryRoutesError};

/// Failure code recorded when a probe amount exceeds what the prober can send
pub const INSUFFICIENT_LOCAL_BALANCE: &str = "INSUFFICIENT_LOCAL_BALANCE";

#[derive(Debug, Clone, Copy)]
pub struct ProbeConfig {
    pub max_fee_sat: u64,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub outcome: ProbeOutcome,
    pub failure_code: Option<String>,
    pub failure_source_index: Option<u32>,
}

/// Classifies a resolved HTLC attempt over a route with `hops` hops.
///
/// Failure source indices count nodes along the route: 0 is the prober's own
/// node and `hops` is the destination (the gateway's LN node).
pub fn classify(attempt: &HtlcAttempt, hops: u32) -> Classification {
    let Some(failure) = attempt
        .failure
        .as_ref()
        .filter(|_| attempt.status == "FAILED")
    else {
        // A probe uses a payment hash with an unknown preimage so it can never
        // succeed, anything else is unexpected.
        return Classification {
            outcome: ProbeOutcome::Error,
            failure_code: Some(format!("UNEXPECTED_STATUS_{}", attempt.status)),
            failure_source_index: None,
        };
    };

    let source = failure.failure_source_index;
    let code = failure.code.as_str();
    let gateway_peer = hops.saturating_sub(1);

    let outcome = if source >= hops {
        // The HTLC made it all the way to the gateway node, which rejected it
        // (usually with INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS): the gateway is
        // reachable and has enough inbound liquidity for the amount.
        ProbeOutcome::Success
    } else if source == gateway_peer {
        // The gateway's channel peer (possibly ourselves when directly
        // connected) could not forward to the gateway.
        match code {
            "UNKNOWN_NEXT_PEER" | "CHANNEL_DISABLED" | "PERMANENT_CHANNEL_FAILURE" => {
                ProbeOutcome::Unreachable
            }
            "TEMPORARY_CHANNEL_FAILURE" if source != 0 => ProbeOutcome::InsufficientLiquidity,
            _ if source == 0 => ProbeOutcome::LocalFailure,
            _ => ProbeOutcome::RouteFailure,
        }
    } else if source == 0 {
        ProbeOutcome::LocalFailure
    } else {
        ProbeOutcome::RouteFailure
    };

    Classification {
        outcome,
        failure_code: Some(code.to_owned()).filter(|code| !code.is_empty()),
        failure_source_index: Some(source),
    }
}

fn result(node_pub_key: &str, amount_msat: u64, outcome: ProbeOutcome) -> GatewayProbeResult {
    GatewayProbeResult {
        node_pub_key: node_pub_key.to_owned(),
        probe_time: Utc::now(),
        amount_msat,
        outcome,
        latency_ms: None,
        failure_code: None,
        failure_source_index: None,
        route_hops: None,
        route_fee_msat: None,
    }
}

/// Sends a single probe of `amount_sat` to `node_pub_key`.
pub async fn probe_once(
    lnd: &LndClient,
    config: ProbeConfig,
    node_pub_key: &str,
    amount_sat: u64,
    max_spendable_msat: u64,
) -> GatewayProbeResult {
    let amount_msat = amount_sat * 1000;
    let mut probe = result(node_pub_key, amount_msat, ProbeOutcome::Error);

    // Don't let our own liquidity show up as a gateway failure
    if amount_msat + config.max_fee_sat * 1000 > max_spendable_msat {
        probe.outcome = ProbeOutcome::LocalFailure;
        probe.failure_code = Some(INSUFFICIENT_LOCAL_BALANCE.to_owned());
        probe.failure_source_index = Some(0);
        return probe;
    }

    let route = match lnd
        .query_routes(node_pub_key, amount_sat, config.max_fee_sat)
        .await
    {
        Ok(route) => route.with_payment_addr(rand::random(), amount_msat),
        Err(QueryRoutesError::NoRoute(message)) => {
            debug!(%node_pub_key, amount_sat, "No route: {message}");
            probe.outcome = ProbeOutcome::NoRoute;
            return probe;
        }
        Err(QueryRoutesError::Other(e)) => {
            warn!(%node_pub_key, amount_sat, "QueryRoutes failed: {e:#}");
            probe.failure_code = Some("QUERY_ROUTES_ERROR".to_owned());
            return probe;
        }
    };
    probe.route_hops = Some(route.hops);
    probe.route_fee_msat = Some(route.total_fees_msat);

    probe.probe_time = Utc::now();
    let started = Instant::now();
    let attempt =
        tokio::time::timeout(config.timeout, lnd.send_to_route(rand::random(), &route)).await;
    let elapsed = started.elapsed();

    match attempt {
        Err(_) => {
            // The HTLC may still be in flight and will resolve eventually,
            // we just don't wait for it anymore.
            probe.outcome = ProbeOutcome::Timeout;
        }
        Ok(Err(e)) => {
            warn!(%node_pub_key, amount_sat, "SendToRoute failed: {e:#}");
            probe.failure_code = Some("SEND_TO_ROUTE_ERROR".to_owned());
        }
        Ok(Ok(attempt)) => {
            let classification = classify(&attempt, route.hops);
            probe.outcome = classification.outcome;
            probe.failure_code = classification.failure_code;
            probe.failure_source_index = classification.failure_source_index;
            probe.latency_ms = Some(
                if attempt.resolve_time_ns > attempt.attempt_time_ns && attempt.attempt_time_ns > 0
                {
                    (attempt.resolve_time_ns - attempt.attempt_time_ns) / 1_000_000
                } else {
                    elapsed.as_millis() as u64
                },
            );
        }
    }
    probe
}

/// Probes `node_pub_key` with ascending amounts, stopping at the first
/// amount that doesn't succeed. The largest successful amount is a lower bound
/// for the gateway's inbound liquidity.
pub async fn probe_ladder(
    lnd: &LndClient,
    config: ProbeConfig,
    node_pub_key: &str,
    amounts_sat: &[u64],
    max_spendable_msat: u64,
) -> Vec<GatewayProbeResult> {
    let mut results = Vec::with_capacity(amounts_sat.len());
    for &amount_sat in amounts_sat {
        let probe = probe_once(lnd, config, node_pub_key, amount_sat, max_spendable_msat).await;
        let succeeded = probe.outcome == ProbeOutcome::Success;
        results.push(probe);
        if !succeeded {
            break;
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lnd::HtlcFailure;

    fn failed(code: &str, source: u32) -> HtlcAttempt {
        HtlcAttempt {
            status: "FAILED".to_owned(),
            attempt_time_ns: 1,
            resolve_time_ns: 2,
            failure: Some(HtlcFailure {
                code: code.to_owned(),
                failure_source_index: source,
            }),
        }
    }

    fn outcome(code: &str, source: u32, hops: u32) -> ProbeOutcome {
        classify(&failed(code, source), hops).outcome
    }

    #[test]
    fn destination_rejection_is_success() {
        assert_eq!(
            outcome("INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS", 3, 3),
            ProbeOutcome::Success
        );
        // Direct channel to the gateway
        assert_eq!(
            outcome("INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS", 1, 1),
            ProbeOutcome::Success
        );
    }

    #[test]
    fn last_hop_failures() {
        assert_eq!(
            outcome("TEMPORARY_CHANNEL_FAILURE", 2, 3),
            ProbeOutcome::InsufficientLiquidity
        );
        assert_eq!(
            outcome("UNKNOWN_NEXT_PEER", 2, 3),
            ProbeOutcome::Unreachable
        );
        assert_eq!(outcome("CHANNEL_DISABLED", 2, 3), ProbeOutcome::Unreachable);
        assert_eq!(
            outcome("FEE_INSUFFICIENT", 2, 3),
            ProbeOutcome::RouteFailure
        );
    }

    #[test]
    fn intermediate_failure_is_route_failure() {
        assert_eq!(
            outcome("TEMPORARY_CHANNEL_FAILURE", 1, 3),
            ProbeOutcome::RouteFailure
        );
    }

    #[test]
    fn own_node_failure_is_local() {
        assert_eq!(
            outcome("TEMPORARY_CHANNEL_FAILURE", 0, 3),
            ProbeOutcome::LocalFailure
        );
        // Directly connected: our own channel lacking balance is on us...
        assert_eq!(
            outcome("TEMPORARY_CHANNEL_FAILURE", 0, 1),
            ProbeOutcome::LocalFailure
        );
        // ...but the gateway being offline is not
        assert_eq!(
            outcome("UNKNOWN_NEXT_PEER", 0, 1),
            ProbeOutcome::Unreachable
        );
    }

    #[test]
    fn records_failure_details() {
        let classification = classify(&failed("TEMPORARY_CHANNEL_FAILURE", 2), 3);
        assert_eq!(
            classification,
            Classification {
                outcome: ProbeOutcome::InsufficientLiquidity,
                failure_code: Some("TEMPORARY_CHANNEL_FAILURE".to_owned()),
                failure_source_index: Some(2),
            }
        );
    }

    #[test]
    fn unexpected_status_is_error() {
        let attempt = HtlcAttempt {
            status: "SUCCEEDED".to_owned(),
            attempt_time_ns: 0,
            resolve_time_ns: 0,
            failure: None,
        };
        let classification = classify(&attempt, 2);
        assert_eq!(classification.outcome, ProbeOutcome::Error);
        assert!(!classification.outcome.is_conclusive());

        let attempt = HtlcAttempt {
            status: "FAILED".to_owned(),
            failure: None,
            ..attempt
        };
        assert_eq!(classify(&attempt, 2).outcome, ProbeOutcome::Error);
    }
}
