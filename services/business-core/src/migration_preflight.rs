use sqlx::postgres::PgPoolOptions;
use std::{collections::BTreeMap, error::Error};

pub(crate) async fn check() -> Result<(), Box<dyn Error>> {
    let url = std::env::var("BUSINESS_CORE_DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query_as::<_, (i64, bool, Vec<u8>)>(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *tx)
    .await?;
    let migrator = sqlx::migrate!("../business-auth-gateway/migrations");
    let embedded = migrator
        .iter()
        .map(|migration| (migration.version, migration.checksum.to_vec()))
        .collect::<BTreeMap<_, _>>();
    validate(&embedded, &rows)?;
    tx.rollback().await?;
    println!(
        "business-migration-preflight: compatible; database head={}, release head={}, pending={}",
        rows.last().map_or(0, |row| row.0),
        embedded.keys().next_back().copied().unwrap_or(0),
        embedded.len() - rows.len()
    );
    Ok(())
}

fn validate(
    embedded: &BTreeMap<i64, Vec<u8>>,
    rows: &[(i64, bool, Vec<u8>)],
) -> Result<(), Box<dyn Error>> {
    if embedded.is_empty() {
        return Err("candidate image has no embedded migrations".into());
    }
    for (index, version) in embedded.keys().enumerate() {
        if *version != index as i64 + 1 {
            return Err(
                format!("candidate migration history is not contiguous at {version}").into(),
            );
        }
    }
    for (index, (version, success, checksum)) in rows.iter().enumerate() {
        let expected = embedded
            .get(version)
            .ok_or_else(|| format!("database version {version} is missing from candidate image"))?;
        if *version != index as i64 + 1 {
            return Err(
                format!("database migration history is not contiguous at {version}").into(),
            );
        }
        if !success {
            return Err(format!("database version {version} is not successfully applied").into());
        }
        if expected != checksum {
            return Err(format!("checksum mismatch for database version {version}").into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use std::collections::BTreeMap;

    #[test]
    fn candidate_accepts_pending_versions_and_rejects_incompatible_history() {
        let embedded = BTreeMap::from([(1, vec![1]), (2, vec![2])]);
        assert!(validate(&embedded, &[]).is_ok());
        assert!(validate(&embedded, &[(1, true, vec![1])]).is_ok());
        assert!(validate(&embedded, &[(1, true, vec![1]), (2, true, vec![2])]).is_ok());
        for rows in [
            vec![(1, true, vec![9])],
            vec![(1, false, vec![1])],
            vec![(2, true, vec![2])],
            vec![(1, true, vec![1]), (2, true, vec![2]), (3, true, vec![3])],
        ] {
            assert!(validate(&embedded, &rows).is_err());
        }
        assert!(validate(&BTreeMap::from([(2, vec![2])]), &[]).is_err());
        assert!(validate(&BTreeMap::new(), &[]).is_err());
    }
}
