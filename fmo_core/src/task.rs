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
