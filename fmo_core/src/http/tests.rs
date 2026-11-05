use axum::body::Body;
use axum::routing::get;
use axum::{middleware, Router};
use tower::ServiceExt;

use super::*;

fn request(path: &str) -> Request {
    Request::builder().uri(path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn rejects_overload_keeps_probes_available_and_recovers() {
    let metrics = Arc::new(HttpMetrics::new(1));
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/federations", get(|| async { "[]" }))
        .layer(middleware::from_fn_with_state(metrics.clone(), admission));
    let permit = metrics.permits.acquire().await.unwrap();
    let rejected = app.clone().oneshot(request("/federations")).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejected.headers()[header::RETRY_AFTER], "1");
    assert_eq!(
        app.clone()
            .oneshot(request("/health"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    drop(permit);
    let served = app.oneshot(request("/federations")).await.unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    assert!(served.headers().contains_key(header::CACHE_CONTROL));
    assert!(served.headers().contains_key("x-request-id"));
    assert_eq!(metrics.requests.load(Ordering::Relaxed), 2);
    assert_eq!(metrics.rejected.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.completed.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn deadline_releases_capacity_and_records_error() {
    let metrics = Arc::new(HttpMetrics::new(1));
    let app = Router::new()
        .route(
            "/slow",
            get(|| async {
                std::future::pending::<()>().await;
                "unreachable"
            }),
        )
        .layer(middleware::from_fn_with_state(metrics.clone(), admission));
    let response = app.oneshot(request("/slow")).await.unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(metrics.permits.available_permits(), 1);
    assert_eq!(metrics.errors.load(Ordering::Relaxed), 1);
}
