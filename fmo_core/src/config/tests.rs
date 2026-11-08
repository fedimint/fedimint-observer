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
    // The cause stays in the chain for the log
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

    // Once the delay has passed, the invite is tried again
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
    // Still claimed by the second request
    assert_eq!(slots.lock().unwrap().len(), 1);
    drop(second);
    assert!(slots.lock().unwrap().is_empty());
}
