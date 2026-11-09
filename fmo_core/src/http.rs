//! Bounded admission, request deadlines, and low-cardinality Prometheus
//! metrics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::AppState;

const BOUNDS_MS: [u64; 8] = [10, 50, 100, 250, 500, 1000, 5000, 30000];

#[derive(Debug)]
pub(crate) struct HttpMetrics {
    permits: Semaphore,
    inspector_permits: Semaphore,
    pub(crate) requests: AtomicU64,
    errors: AtomicU64,
    rejected: AtomicU64,
    elapsed_us: AtomicU64,
    completed: AtomicU64,
    buckets: [AtomicU64; 8],
}

impl HttpMetrics {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            permits: Semaphore::new(limit),
            inspector_permits: Semaphore::new(4),
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            elapsed_us: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            buckets: Default::default(),
        }
    }
}

pub(crate) async fn admission(
    State(metrics): State<Arc<HttpMetrics>>,
    request: Request,
    next: Next,
) -> Response {
    // Probes must remain usable while application request capacity is exhausted.
    if matches!(
        request.uri().path(),
        "/health" | "/live" | "/ready" | "/metrics"
    ) {
        return next.run(request).await;
    }
    let id = metrics.requests.fetch_add(1, Ordering::Relaxed) + 1;
    let Ok(_permit) = metrics.permits.try_acquire() else {
        metrics.rejected.fetch_add(1, Ordering::Relaxed);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::RETRY_AFTER, "1")],
            "Server is busy",
        )
            .into_response();
    };
    let start = Instant::now();
    let _inspector = if request.uri().path().starts_with("/config/") {
        match metrics.inspector_permits.try_acquire() {
            Ok(permit) => Some(permit),
            Err(_) => {
                metrics.rejected.fetch_add(1, Ordering::Relaxed);
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::RETRY_AFTER, "5")],
                    "Inspector is busy",
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let cacheable = request.method() == axum::http::Method::GET
        && request.uri().path().starts_with("/federations");
    let mut response = match tokio::time::timeout(
        Duration::from_secs(30),
        next.run(request)
            .instrument(tracing::info_span!("request", request_id = id)),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => (StatusCode::GATEWAY_TIMEOUT, "Request timed out").into_response(),
    };
    let elapsed = start.elapsed();
    metrics
        .elapsed_us
        .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
    metrics.completed.fetch_add(1, Ordering::Relaxed);
    for (bound, count) in BOUNDS_MS.iter().zip(&metrics.buckets) {
        if elapsed.as_millis() <= u128::from(*bound) {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }
    if response.status().is_server_error() {
        metrics.errors.fetch_add(1, Ordering::Relaxed);
    }
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&id.to_string()).expect("integer is a header"),
    );
    if cacheable && response.status().is_success() {
        response
            .headers_mut()
            .entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static(
                "public, max-age=10, stale-while-revalidate=30",
            ));
    }
    response
}

pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    let observer = &state.federation_observer;
    if !observer.healthy() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let check = async {
        observer
            .connection()
            .await?
            .simple_query("SELECT 1")
            .await?;
        anyhow::Ok(())
    };
    match tokio::time::timeout(Duration::from_secs(2), check).await {
        Ok(Ok(())) => (StatusCode::OK, "ready").into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

pub(crate) async fn metrics(State(state): State<AppState>) -> Response {
    let metrics = &state.http_metrics;
    let read = |value: &AtomicU64| value.load(Ordering::Relaxed);
    let total = read(&metrics.requests);
    let rejected = read(&metrics.rejected);
    let completed = read(&metrics.completed);
    let mut body = format!("# TYPE fmo_http_requests_total counter\nfmo_http_requests_total {total}\n# TYPE fmo_http_rejected_total counter\nfmo_http_rejected_total {rejected}\n# TYPE fmo_http_errors_total counter\nfmo_http_errors_total {}\n# TYPE fmo_http_duration_seconds histogram\n", read(&metrics.errors));
    for (bound, count) in BOUNDS_MS.iter().zip(&metrics.buckets) {
        body.push_str(&format!(
            "fmo_http_duration_seconds_bucket{{le=\"{}\"}} {}\n",
            *bound as f64 / 1000.0,
            read(count)
        ));
    }
    body.push_str(&format!("fmo_http_duration_seconds_bucket{{le=\"+Inf\"}} {completed}\nfmo_http_duration_seconds_count {completed}\nfmo_http_duration_seconds_sum {}\n", read(&metrics.elapsed_us) as f64 / 1_000_000.0));
    let (api, worker) = state.federation_observer.pool_status();
    for (label, status) in [("api", api), ("worker", worker)] {
        body.push_str(&format!("fmo_db_pool_size{{pool=\"{label}\"}} {}\nfmo_db_pool_available{{pool=\"{label}\"}} {}\nfmo_db_pool_waiting{{pool=\"{label}\"}} {}\n", status.size, status.available, status.waiting));
    }
    body.push_str(&format!(
        "fmo_workers_healthy {}\n",
        u8::from(state.federation_observer.healthy())
    ));
    (
        [
            (header::CONTENT_TYPE, "text/plain; version=0.0.4"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

pub(crate) async fn shutdown_signal(observer: crate::federation::observer::FederationObserver) {
    let unhealthy = observer.task_group().make_handle().make_shutdown_rx();
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate => {}, _ = unhealthy => {} }
    if let Err(error) = observer.shutdown().await {
        tracing::warn!(%error, "Worker shutdown failed");
    }
}

#[cfg(test)]
mod tests;
