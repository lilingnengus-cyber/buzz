use std::{collections::BTreeMap, fs, path::Path};

const EXPECTED_MIGRATION_HEAD: i64 = 76;

#[test]
fn migration_versions_are_unique_and_contiguous() {
    let migrations_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../business-auth-gateway/migrations");
    let mut files_by_version = BTreeMap::new();

    for entry in fs::read_dir(&migrations_path).expect("migration directory must be readable") {
        let entry = entry.expect("migration directory entry must be readable");
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.ends_with(".sql") {
            continue;
        }

        let version = file_name
            .split_once('_')
            .expect("migration file name must contain an underscore")
            .0
            .parse::<i64>()
            .expect("migration file name must start with a numeric version");
        assert!(
            files_by_version
                .insert(version, file_name.into_owned())
                .is_none(),
            "migration version {version} is duplicated"
        );
    }

    let versions = files_by_version.keys().copied().collect::<Vec<_>>();
    assert_eq!(
        versions,
        (1..=EXPECTED_MIGRATION_HEAD).collect::<Vec<_>>(),
        "migration versions must be contiguous through the canonical head; update \
         EXPECTED_MIGRATION_HEAD when adding a migration"
    );
}
