use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::supervise;

#[tokio::test(start_paused = true)]
async fn restarts_after_errors_and_panics() {
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    let supervisor = tokio::spawn(async move {
        supervise("test task", move || {
            let run = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                match run {
                    0 => panic!("boom"),
                    1 => anyhow::bail!("failed"),
                    _ => std::future::pending().await,
                }
            }
        })
        .await
    });

    // Paused time auto-advances through both 30 s restart delays
    while runs.load(Ordering::SeqCst) < 3 {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(
        !supervisor.is_finished(),
        "a panic must not end the supervisor"
    );
    supervisor.abort();
}
