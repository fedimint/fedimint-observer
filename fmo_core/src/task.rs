//! Keeping background tasks alive

use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use futures::FutureExt;
use tracing::error;

const RESTART_DELAY: Duration = Duration::from_secs(30);

/// Runs `task` forever, restarting it 30 s after it fails, ends or panics.
///
/// A panic must never escape a task spawned on fedimint's `TaskGroup`: its
/// panic guard shuts the whole group down, which stops every federation's
/// observer and monitors at once while the API keeps serving stale data.
pub(crate) async fn supervise<F, Fut>(name: &str, mut task: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    loop {
        let error = match AssertUnwindSafe(task()).catch_unwind().await {
            Ok(Ok(())) => anyhow::anyhow!("{name} exited unexpectedly"),
            Ok(Err(error)) => error,
            Err(panic) => anyhow::anyhow!("panicked: {}", panic_message(&*panic)),
        };
        error!("{name} errored, restarting in 30s: {error:#}");
        tokio::time::sleep(RESTART_DELAY).await;
    }
}

fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic payload")
}

#[cfg(test)]
mod tests {
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
}
