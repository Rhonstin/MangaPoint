//! Downloading page images reliably.

use catalog::{Catalog, CatalogError};
use std::future::Future;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// Total tries per request, including the first.
    pub attempts: u32,
    /// Wait before the second try; doubles each time (capped at 5 s).
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            base_delay: Duration::from_millis(400),
        }
    }
}

/// Worth trying again? Rate limits, server errors and network hiccups are;
/// a missing page, or a source that needs the browser, is not.
pub fn is_retryable(err: &CatalogError) -> bool {
    match err {
        CatalogError::Http(_) | CatalogError::Io(_) => true,
        CatalogError::SourceHttp(code) => *code == 429 || (500..600).contains(code),
        _ => false,
    }
}

/// Run `op` up to `policy.attempts` times, waiting longer after each failure.
pub async fn with_retry<T, F, Fut>(policy: &RetryPolicy, mut op: F) -> Result<T, CatalogError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, CatalogError>>,
{
    let attempts = policy.attempts.max(1);
    let mut delay = policy.base_delay;
    let mut attempt = 1;
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < attempts && is_retryable(&e) => {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(5));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

pub async fn fetch_page<C: Catalog>(
    catalog: &C,
    path: &str,
    policy: &RetryPolicy,
) -> Result<Vec<u8>, CatalogError> {
    with_retry(policy, || catalog.get_bytes(path)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::FakeCatalog;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn quick(attempts: u32) -> RetryPolicy {
        RetryPolicy {
            attempts,
            base_delay: Duration::from_millis(1),
        }
    }

    #[tokio::test]
    async fn retries_transient_errors_until_they_clear() {
        let c = FakeCatalog::new();
        c.set_bytes("/p", vec![1, 2, 3]);
        c.flaky_path("/p", 2); // fails twice, then works
        assert_eq!(
            fetch_page(&c, "/p", &quick(3)).await.unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(c.calls().iter().filter(|x| *x == "get /p").count(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_the_last_attempt() {
        let c = FakeCatalog::new();
        c.set_bytes("/p", vec![1]);
        c.flaky_path("/p", 5);
        let err = fetch_page(&c, "/p", &quick(3)).await.unwrap_err();
        assert!(matches!(err, CatalogError::SourceHttp(503)));
        assert_eq!(c.calls().iter().filter(|x| *x == "get /p").count(), 3);
    }

    #[tokio::test]
    async fn does_not_retry_a_missing_page() {
        let c = FakeCatalog::new();
        let err = fetch_page(&c, "/gone", &quick(5)).await.unwrap_err();
        assert!(matches!(err, CatalogError::SourceHttp(404)));
        assert_eq!(c.calls().len(), 1, "404 is final");
    }

    #[test]
    fn retry_classification() {
        assert!(is_retryable(&CatalogError::SourceHttp(503)));
        assert!(is_retryable(&CatalogError::SourceHttp(429)));
        assert!(!is_retryable(&CatalogError::SourceHttp(404)));
        assert!(!is_retryable(&CatalogError::SourceHttp(403)));
        assert!(!is_retryable(&CatalogError::NeedsBrowser));
        assert!(!is_retryable(&CatalogError::NeedsFlareSolverr));
    }

    #[tokio::test]
    async fn backoff_waits_between_attempts() {
        let calls = AtomicU32::new(0);
        let policy = RetryPolicy {
            attempts: 3,
            base_delay: Duration::from_millis(20),
        };
        let start = std::time::Instant::now();
        let r: Result<(), CatalogError> = with_retry(&policy, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(CatalogError::SourceHttp(500)) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // 20 ms then 40 ms.
        assert!(
            start.elapsed() >= Duration::from_millis(55),
            "{:?}",
            start.elapsed()
        );
    }
}
