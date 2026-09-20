use super::*;
impl drafts::Environment<'_> {
    pub(super) async fn verify_reversal(&self) -> i64 {
        sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,'profit_adjustment:reverse' FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(self.fixture.actor).execute(self.pool).await.unwrap();
        sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES('profit_adjustment:reverse','profit_adjustment:reverse',ARRAY['b2_operator'],1,true)").execute(self.pool).await.unwrap();
        let kind = "operational_adjustment_reversal_intent";
        let scope = "operational_adjustment_reversal_intent:approve";
        for mode in ["approve", "reject", "pending", "stale", "hash", "revoked"] {
            let batch = fixture::draft(
                self.pool,
                self.fixture,
                self.order,
                &format!("reverse-chain-{mode}"),
            )
            .await;
            let service = business_core::b4::AdjustmentService::new(
                PgStore::new(self.pool.clone()),
                "ADJ".into(),
                500,
            );
            let version = business_core::b4::model::VersionCommand {
                expected_version: 1,
            };
            let preview = service
                .allocation_preview(self.fixture.actor, batch, &version)
                .await
                .unwrap();
            service
                .post_guarded(
                    self.fixture.actor,
                    Uuid::new_v4(),
                    batch,
                    &format!("reverse-chain-post-{mode}"),
                    &version,
                    &preview,
                )
                .await
                .unwrap();
            let p = self
                .prepare(
                    kind,
                    json!({"batchId":batch,"expectedVersion":3,"reason":"重复费用测试"}),
                )
                .await;
            assert_eq!(batch_status(self.pool, batch).await, "posted");
            if matches!(mode, "approve" | "reject" | "pending") {
                let minimum: i16 = if mode == "pending" { 2 } else { 1 };
                sqlx::query("UPDATE business_approval_policies SET min_approvers=$1 WHERE action_code='profit_adjustment:reverse'").bind(minimum).execute(self.pool).await.unwrap();
                let result = self
                    .approve(
                        kind,
                        &p,
                        if mode == "reject" {
                            "reject"
                        } else {
                            "approve"
                        },
                    )
                    .await;
                assert_eq!(result["executed"], mode == "approve");
                assert_eq!(
                    result["status"],
                    if mode == "approve" {
                        "executed"
                    } else if mode == "reject" {
                        "rejected"
                    } else {
                        "pending"
                    }
                );
                assert_eq!(
                    batch_status(self.pool, batch).await,
                    if mode == "approve" {
                        "reversed"
                    } else {
                        "posted"
                    }
                );
                if mode == "approve" {
                    assert_eq!(result["reversedDocument"]["version"], 4);
                    let sum:String=sqlx::query_scalar("SELECT sum(CASE direction WHEN 'normal' THEN amount ELSE -amount END)::text FROM profit_facts WHERE source_type='operational_adjustment' AND source_id=$1").bind(batch).fetch_one(self.pool).await.unwrap();
                    assert_eq!(sum, "0.000000");
                } else {
                    assert!(result["reversedDocument"].is_null());
                }
                sqlx::query("UPDATE business_approval_policies SET min_approvers=1 WHERE action_code='profit_adjustment:reverse'").execute(self.pool).await.unwrap();
            } else {
                let command = if mode == "hash" {
                    p["approvalCommand"]
                        .as_str()
                        .unwrap()
                        .replace(p["previewHash"].as_str().unwrap(), &"0".repeat(64))
                } else {
                    p["approvalCommand"].as_str().unwrap().into()
                };
                let issued = issue(self.gateway, self.credential, self.keys, &command, scope).await;
                if mode == "revoked" {
                    let response = reqwest::Client::new()
                        .post(
                            self.gateway
                                .join(&format!(
                                    "internal/agent-delegations/{}/revoke",
                                    issued["id"].as_str().unwrap()
                                ))
                                .unwrap(),
                        )
                        .header("x-business-service-credential", self.credential)
                        .send()
                        .await
                        .unwrap();
                    assert!(response.status().is_success());
                }
                if mode == "stale" {
                    sqlx::query(
                        "UPDATE operational_adjustment_batches SET version=version+1 WHERE id=$1",
                    )
                    .bind(batch)
                    .execute(self.pool)
                    .await
                    .unwrap();
                }
                let before = self.footprint().await;
                let mut client = mcp::Client::start(
                    self.binary,
                    self.gateway,
                    self.api,
                    self.credential,
                    &issued,
                    Some(scope),
                )
                .await;
                let result = client
                    .call("approve_operational_adjustment_reversal", json!({}))
                    .await;
                assert_ne!(result["executed"], true, "{mode}: {result}");
                assert_ne!(result["status"], "pending", "{mode}: {result}");
                client.stop().await;
                assert_eq!(self.footprint().await, before, "{mode}");
            }
        }
        9
    }
}
