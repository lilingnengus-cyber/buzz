//! Bounded retry for aborted immutable-report transactions.
use crate::b2::DomainError;

// Only retry transactions PostgreSQL guarantees were aborted. Every attempt
// opens a fresh snapshot and rechecks current authority; business errors escape.
pub(crate) async fn retry<F, Fut, T>(mut operation: F) -> Result<T, DomainError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, DomainError>>,
{
    let mut retries = 0;
    loop {
        match operation().await {
            Err(DomainError::Database(sqlx::Error::Database(ref error)))
                if retries < 2 && matches!(error.code().as_deref(), Some("40001" | "40P01")) =>
            {
                retries += 1;
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}
