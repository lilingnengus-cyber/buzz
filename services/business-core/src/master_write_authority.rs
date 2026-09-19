//! Final authority check after resource waits, held until the write commits.
use crate::{b2::common::DomainError, model::AuthorizationSnapshot, store::PgStore};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Re-read current write permission while holding the authorization revision lock.
pub(crate) async fn snapshot(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    permission: &str,
    grants_scope: bool,
) -> Result<AuthorizationSnapshot, DomainError> {
    // Creation grants creator scope. Take the stronger lock immediately instead
    // of deadlocking two concurrent SHARE-to-UPDATE upgrades in scope triggers.
    let query = if grants_scope {
        "SELECT revision FROM business_authorization_revision WHERE singleton FOR UPDATE"
    } else {
        "SELECT revision FROM business_authorization_revision WHERE singleton FOR SHARE"
    };
    sqlx::query(query).fetch_one(&mut **tx).await?;
    read(tx, actor, permission).await
}

/// Read preliminary permission using the existing transaction connection.
/// Writers still acquire the revision lock and recheck after resource waits.
pub(crate) async fn read(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    permission: &str,
) -> Result<AuthorizationSnapshot, DomainError> {
    let current = PgStore::snapshot_on(tx, actor)
        .await
        .map_err(|_| DomainError::NotFoundOrForbidden)?;
    if !current.permission_keys.contains(permission) {
        return Err(DomainError::NotFoundOrForbidden);
    }
    Ok(current)
}
