use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use futures::FutureExt;
use tracing::error;

const RESTART_DELAY: Duration = Duration::from_secs(30);

/// Restarts a task after errors, exits or panics.
/// Panics are caught so one task cannot stop every observer.
pub(crate) async fn supervise<F, Fut>(name: impl Into<String>, mut task: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let name = name.into();
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
mod tests;
