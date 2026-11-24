use axum::http::StatusCode;

use super::*;

fn status(error: &anyhow::Error) -> StatusCode {
    error
        .downcast_ref::<ClientError>()
        .expect("a client error")
        .status()
}

#[test]
fn config_downloads_fail_with_gateway_statuses() {
    let timed_out = download_outcome::<()>(None).unwrap_err();
    assert_eq!(status(&timed_out), StatusCode::GATEWAY_TIMEOUT);

    let unreachable = download_outcome::<()>(Some(Err(anyhow!("connection refused")))).unwrap_err();
    assert_eq!(status(&unreachable), StatusCode::BAD_GATEWAY);
    assert!(format!("{unreachable:#}").contains("connection refused"));

    let busy =
        download_outcome::<()>(Some(Err(ClientError::unavailable("busy").into()))).unwrap_err();
    assert_eq!(status(&busy), StatusCode::SERVICE_UNAVAILABLE);

    assert_eq!(download_outcome(Some(Ok(1))).unwrap(), 1);
}

#[test]
fn failed_invites_are_not_retried_right_away() {
    let cache = FederationConfigCache::default();
    assert!(cache.recent_failure("invite").is_none());

    cache.record_failure("invite".to_owned(), ClientError::bad_gateway("down"));
    let failure = cache.recent_failure("invite").expect("remembered");
    assert_eq!(failure.status(), StatusCode::BAD_GATEWAY);
    assert!(cache.recent_failure("other").is_none());

    cache.failures.lock().unwrap().get_mut("invite").unwrap().0 =
        Instant::now() - FAILED_INVITE_RETRY_DELAY;
    assert!(cache.recent_failure("invite").is_none());
    assert!(cache.failures.lock().unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_downloads_of_an_invite_share_a_slot() {
    let slots = DownloadSlots::default();
    let first = DownloadSlot::take(&slots, "invite");
    let second = DownloadSlot::take(&slots, "invite");
    assert!(Arc::ptr_eq(&first.slot, &second.slot));

    let guard = first.lock().await;
    assert!(second.try_lock().is_err());
    drop(guard);
    drop(first);
    assert_eq!(slots.lock().unwrap().len(), 1);
    drop(second);
    assert!(slots.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unavailable_announced_route_falls_back_to_literal_invite_route() {
    let federation_id = endpoints_config(BTreeMap::new()).calculate_federation_id();
    let result = prefer_announced_route(
        federation_id,
        "test invite lookup",
        || Ok(async { Err::<&str, _>(anyhow!("announced route unavailable")) }),
        || Ok(async { Ok("literal invite route") }),
    )
    .await
    .expect("the literal route remains a bounded fallback");

    assert_eq!(result, "literal invite route");
}

#[tokio::test]
async fn literal_invite_route_is_constructed_after_announced_client_failure() {
    let federation_id = endpoints_config(BTreeMap::new()).calculate_federation_id();
    let result = prefer_announced_route(
        federation_id,
        "test invite lookup",
        || {
            Err::<std::future::Ready<anyhow::Result<&str>>, _>(anyhow!(
                "announced client construction failed"
            ))
        },
        || {
            Ok(std::future::ready(Ok(
                "independently constructed literal route",
            )))
        },
    )
    .await
    .expect("literal client construction must be independent");

    assert_eq!(result, "independently constructed literal route");
}

#[tokio::test]
async fn working_announced_route_does_not_construct_broken_literal_client() {
    let federation_id = endpoints_config(BTreeMap::new()).calculate_federation_id();
    let result = prefer_announced_route(
        federation_id,
        "test invite lookup",
        || Ok(std::future::ready(Ok("working announced route"))),
        || -> anyhow::Result<std::future::Ready<anyhow::Result<&str>>> {
            panic!("literal client must be lazy when the announced route works")
        },
    )
    .await
    .expect("the announced route succeeds independently");

    assert_eq!(result, "working announced route");
}
