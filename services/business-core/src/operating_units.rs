use crate::b2::common::DomainError;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Verifies that an operating unit can be placed below the proposed active
/// parent without creating a cycle.
pub async fn validate_parent(
    tx: &mut Transaction<'_, Postgres>,
    unit_id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<(), DomainError> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    if parent_id == unit_id {
        return Err(DomainError::Invalid("OPERATING_UNIT_CYCLE".into()));
    }
    let parent_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM business_units WHERE id=$1 AND status='active')",
    )
    .bind(parent_id)
    .fetch_one(&mut **tx)
    .await?;
    if !parent_active {
        return Err(DomainError::NotFoundOrForbidden);
    }
    let cycle: bool = sqlx::query_scalar(
        "WITH RECURSIVE ancestors AS (
            SELECT id,parent_business_unit_id FROM business_units WHERE id=$1
            UNION
            SELECT parent.id,parent.parent_business_unit_id
            FROM business_units parent
            JOIN ancestors child ON child.parent_business_unit_id=parent.id
         )
         SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=$2)",
    )
    .bind(parent_id)
    .bind(unit_id)
    .fetch_one(&mut **tx)
    .await?;
    if cycle {
        Err(DomainError::Invalid("OPERATING_UNIT_CYCLE".into()))
    } else {
        Ok(())
    }
}

/// Expands operating-unit roots to a deduplicated set containing every
/// descendant. Disabled nodes are either retained for historical reads or
/// omitted for new-write selection according to `include_disabled`.
pub async fn descendant_ids<'e, E>(
    executor: E,
    roots: &BTreeSet<Uuid>,
    include_disabled: bool,
) -> Result<BTreeSet<Uuid>, DomainError>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    if roots.is_empty() {
        return Ok(BTreeSet::new());
    }
    let rows = sqlx::query_scalar::<_, Uuid>(
        "WITH RECURSIVE descendants AS (
            SELECT id FROM business_units
            WHERE id=ANY($1) AND ($2 OR status='active')
            UNION
            SELECT child.id FROM business_units child
            JOIN descendants parent ON child.parent_business_unit_id=parent.id
            WHERE $2 OR child.status='active'
         )
         SELECT id FROM descendants ORDER BY id",
    )
    .bind(roots.iter().copied().collect::<Vec<_>>())
    .bind(include_disabled)
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Returns whether an operating unit has at least one active descendant.
pub async fn has_active_descendants(pool: &PgPool, id: Uuid) -> Result<bool, DomainError> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE descendants AS (
            SELECT child.id,child.status FROM business_units child
            WHERE child.parent_business_unit_id=$1
            UNION
            SELECT child.id,child.status FROM business_units child
            JOIN descendants parent ON child.parent_business_unit_id=parent.id
         )
         SELECT EXISTS(SELECT 1 FROM descendants WHERE status='active')",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}
