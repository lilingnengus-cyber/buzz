use life_auth_gateway::{
    model::{LifeWorkbenchUserId, WorkbenchSessionId},
    write_confirmation::{parse_exact_confirmation, ValidateWriteConfirmationRequest},
    Store,
};
use nostr::{EventBuilder, Keys, Kind, Timestamp};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    AssertSqlSafe, PgPool, Row,
};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;

struct Database {
    admin: PgPool,
    pool: PgPool,
    schema: String,
}

impl Database {
    async fn create() -> Option<Self> {
        let url = std::env::var("LIFE_AUTH_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;
        let schema = format!("life_auth_test_{}", Uuid::new_v4().simple());
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\"")))
            .execute(&admin)
            .await
            .ok()?;
        let options = PgConnectOptions::from_str(&url)
            .ok()?
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(12)
            .connect_with(options)
            .await
            .ok()?;
        Store::migrate(&pool).await.ok()?;
        Some(Self {
            admin,
            pool,
            schema,
        })
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP SCHEMA \"{}\" CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await
        .expect("drop test schema");
        self.admin.close().await;
    }
}

async fn seed(pool: &PgPool, keys: &Keys) -> (LifeWorkbenchUserId, WorkbenchSessionId) {
    let user = Uuid::new_v4();
    let session = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO life_workbench_users
         (id,oidc_issuer,oidc_subject,life_os_user_id,status)
         VALUES($1,'https://identity.example','writer','life-writer','active')",
    )
    .bind(user)
    .execute(pool)
    .await
    .expect("user");
    sqlx::query(
        "INSERT INTO life_workbench_sessions
         (id,workbench_user_id,deployment_id,token_hash,status,expires_at)
         VALUES($1,$2,'life-test',$3,'active',now()+interval '1 hour')",
    )
    .bind(session)
    .bind(user)
    .bind(Sha256::digest(b"writer-session").to_vec())
    .execute(pool)
    .await
    .expect("session");
    sqlx::query(
        "INSERT INTO life_identity_bindings
         (id,workbench_user_id,buzz_pubkey,source_event_id,status)
         VALUES($1,$2,$3,$4,'active')",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(keys.public_key().to_hex())
    .bind("e".repeat(64))
    .execute(pool)
    .await
    .expect("binding");
    (
        LifeWorkbenchUserId::new(user),
        WorkbenchSessionId::new(session),
    )
}

fn command(command_id: Uuid, version: i64, hash: &str) -> String {
    format!("/confirm life-write {command_id} v{version} {hash}")
}

#[test]
fn parser_accepts_only_the_canonical_confirmation_command() {
    let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("UUID");
    let hash = "a".repeat(64);
    let parsed = parse_exact_confirmation(&command(id, 7, &hash)).expect("exact command");
    assert_eq!(parsed.command_id, id);
    assert_eq!(parsed.expected_version, 7);
    assert_eq!(parsed.preview_hash, hash);
    for invalid in [
        "确认".to_string(),
        format!(" /confirm life-write {id} v7 {}", "a".repeat(64)),
        format!("/confirm  life-write {id} v7 {}", "a".repeat(64)),
        format!("/confirm life-write {id} v07 {}", "a".repeat(64)),
        format!("/confirm life-write {id} v0 {}", "a".repeat(64)),
        format!("/confirm life-write {id} v7 {} extra", "a".repeat(64)),
        format!("/confirm life-write {} v7 {}", id.simple(), "a".repeat(64)),
        format!("/confirm life-write {id} v7 {}", "A".repeat(64)),
    ] {
        assert!(
            parse_exact_confirmation(&invalid).is_err(),
            "accepted: {invalid}"
        );
    }
}

#[tokio::test]
async fn signed_fresh_confirmation_binds_fields_and_consumes_once() {
    let Some(database) = Database::create().await else {
        eprintln!("LIFE_AUTH_TEST_DATABASE_URL absent; confirmation test skipped");
        return;
    };
    let keys = Keys::generate();
    let (user, session) = seed(&database.pool, &keys).await;
    let store = Store::new(database.pool.clone());
    let command_id = Uuid::new_v4();
    let preview_hash = "a".repeat(64);
    let event = EventBuilder::new(Kind::TextNote, command(command_id, 7, &preview_hash))
        .sign_with_keys(&keys)
        .expect("signed event");
    let request = ValidateWriteConfirmationRequest {
        signed_event: event.clone(),
        command_id,
        expected_version: 7,
        preview_hash: preview_hash.clone(),
        trace_id: Uuid::new_v4(),
    };
    assert!(store
        .validate_write_confirmation(
            request.clone(),
            "other-deployment",
            Duration::from_secs(600),
        )
        .await
        .is_err());
    let validated = store
        .validate_write_confirmation(request, "life-test", Duration::from_secs(600))
        .await
        .expect("validate confirmation");
    assert_eq!(validated.command_id, command_id);
    assert_eq!(validated.user_id, user);
    assert_eq!(validated.workbench_session_id, session);
    let ttl = validated.expires_at - chrono::Utc::now();
    assert!(ttl.num_seconds() >= 598 && ttl.num_seconds() <= 600);
    let row = sqlx::query(
        "SELECT source_event_id,expected_version,preview_hash,status
         FROM life_write_command_confirmations WHERE id=$1",
    )
    .bind(validated.confirmation_id.as_uuid())
    .fetch_one(&database.pool)
    .await
    .expect("confirmation row");
    assert_eq!(row.get::<String, _>("source_event_id"), event.id.to_hex());
    assert_eq!(row.get::<i64, _>("expected_version"), 7);
    assert_eq!(
        row.get::<Vec<u8>, _>("preview_hash"),
        hex::decode(&preview_hash).unwrap()
    );
    let consumed = store
        .consume_write_confirmation(command_id, user, session, &event.id.to_hex(), 7)
        .await
        .expect("consume confirmation");
    assert_eq!(consumed.confirmation_id, validated.confirmation_id);
    assert_eq!(consumed.preview_hash, preview_hash);
    assert!(store
        .consume_write_confirmation(command_id, user, session, &event.id.to_hex(), 7)
        .await
        .is_err());
    database.cleanup().await;
}

#[tokio::test]
async fn signature_freshness_author_and_command_fields_fail_closed() {
    let Some(database) = Database::create().await else {
        return;
    };
    let keys = Keys::generate();
    seed(&database.pool, &keys).await;
    let store = Store::new(database.pool.clone());
    let validate =
        |event, command_id, version, preview_hash: String| ValidateWriteConfirmationRequest {
            signed_event: event,
            command_id,
            expected_version: version,
            preview_hash,
            trace_id: Uuid::new_v4(),
        };
    let id = Uuid::new_v4();
    let hash = "b".repeat(64);
    let unbound = Keys::generate();
    let wrong_author = EventBuilder::new(Kind::TextNote, command(id, 3, &hash))
        .sign_with_keys(&unbound)
        .unwrap();
    assert!(store
        .validate_write_confirmation(
            validate(wrong_author, id, 3, hash.clone()),
            "life-test",
            Duration::from_secs(600)
        )
        .await
        .is_err());
    let old = EventBuilder::new(Kind::TextNote, command(id, 3, &hash))
        .custom_created_at(Timestamp::from_secs(
            u64::try_from(chrono::Utc::now().timestamp() - 601).unwrap(),
        ))
        .sign_with_keys(&keys)
        .unwrap();
    assert!(store
        .validate_write_confirmation(
            validate(old, id, 3, hash.clone()),
            "life-test",
            Duration::from_secs(600),
        )
        .await
        .is_err());
    let wrong_kind = EventBuilder::new(Kind::Custom(24243), command(id, 3, &hash))
        .sign_with_keys(&keys)
        .unwrap();
    assert!(store
        .validate_write_confirmation(
            validate(wrong_kind, id, 3, hash.clone()),
            "life-test",
            Duration::from_secs(600)
        )
        .await
        .is_err());
    for (body_id, request_id, body_version, request_version, request_hash) in [
        (id, Uuid::new_v4(), 3, 3, hash.clone()),
        (id, id, 3, 4, hash.clone()),
        (id, id, 3, 3, "c".repeat(64)),
    ] {
        let event = EventBuilder::new(Kind::TextNote, command(body_id, body_version, &hash))
            .sign_with_keys(&keys)
            .unwrap();
        assert!(store
            .validate_write_confirmation(
                validate(event, request_id, request_version, request_hash),
                "life-test",
                Duration::from_secs(600),
            )
            .await
            .is_err());
    }
    database.cleanup().await;
}
