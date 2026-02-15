use std::future::Future;
use std::time::Duration;
use tracing;

/// Retry a fallible async operation with exponential backoff.
pub async fn with_retry<F, Fut, T, E>(
    max_retries: u32,
    base_delay_ms: u64,
    operation_name: &str,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt > max_retries {
                    tracing::error!(
                        operation = operation_name,
                        attempts = attempt,
                        "All retry attempts exhausted"
                    );
                    return Err(e);
                }

                let delay = Duration::from_millis(base_delay_ms * 2u64.pow(attempt - 1));
                tracing::warn!(
                    operation = operation_name,
                    attempt = attempt,
                    max_retries = max_retries,
                    delay_ms = delay.as_millis() as u64,
                    error = %e,
                    "Retrying after failure"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}
