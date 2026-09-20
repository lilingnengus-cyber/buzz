//! Retry only PostgreSQL-aborted transactions, always with fresh authority and preview.
use super::StoreError;

pub(super) async fn run<F, Fut, T>(mut operation: F) -> Result<T, StoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, StoreError>>,
{
    let mut retries = 0;
    loop {
        match operation().await {
            Err(StoreError::Database(sqlx::Error::Database(ref error)))
                if retries < 2 && matches!(error.code().as_deref(), Some("40001" | "40P01")) =>
            {
                retries += 1;
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}
