use super::*;
pub(super) struct Environment<'a> {
    pub pool: &'a PgPool,
    pub binary: &'a str,
    pub gateway: &'a Url,
    pub api: &'a Url,
    pub credential: &'a str,
    pub keys: &'a Keys,
    pub fixture: &'a crate::test_fixture::Fixture,
    pub order: Uuid,
}
impl Environment<'_> {
    fn batch(&self) -> Value {
        let f = self.fixture;
        json!({"legalEntityId":f.legal_entity,"currency":"CNY","managementPeriod":"2026-08","lines":[{"metricType":"allocated_operating_expense","amount":"10.01","businessDate":"2026-08-21","allocationBasis":"direct","directSalesOrderId":self.order,"customerId":f.customer,"businessUnitId":f.business_unit,"brandId":f.brand,"warehouseId":f.warehouse,"reasonCode":"TEST"}]})
    }
    pub(super) async fn call(
        &self,
        tool: &str,
        scope: &str,
        message: &str,
        input: Value,
    ) -> (Value, Value) {
        let issued = issue(self.gateway, self.credential, self.keys, message, scope).await;
        let mut client = mcp::Client::start(
            self.binary,
            self.gateway,
            self.api,
            self.credential,
            &issued,
            scope.ends_with(":approve").then_some(scope),
        )
        .await;
        let result = client.call(tool, input).await;
        client.stop().await;
        (result, issued)
    }
    pub(super) async fn prepare(&self, kind: &str, input: Value) -> Value {
        let suffix = kind.strip_suffix("_intent").unwrap();
        let (prepared, _) = self
            .call(
                &format!("prepare_{suffix}"),
                &format!("{kind}:create"),
                "准备费用草稿，等待明确确认",
                input,
            )
            .await;
        assert_eq!(prepared["status"], "ok", "{prepared}");
        prepared
    }
    pub(super) async fn approve(&self, kind: &str, p: &Value, decision: &str) -> Value {
        let suffix = kind.strip_suffix("_intent").unwrap();
        let command = p[if decision == "approve" {
            "approvalCommand"
        } else {
            "rejectionCommand"
        }]
        .as_str()
        .unwrap();
        let issued = issue(
            self.gateway,
            self.credential,
            self.keys,
            command,
            &format!("{kind}:approve"),
        )
        .await;
        let mut client = mcp::Client::start(
            self.binary,
            self.gateway,
            self.api,
            self.credential,
            &issued,
            Some(&format!("{kind}:approve")),
        )
        .await;
        let result = client.call(&format!("approve_{suffix}"), json!({})).await;
        assert!(
            matches!(
                result["status"].as_str(),
                Some("executed" | "rejected" | "pending")
            ),
            "{result}"
        );
        let repeat = client.call(&format!("approve_{suffix}"), json!({})).await;
        assert_ne!(repeat["executed"], true);
        client.stop().await;
        let source: String = sqlx::query_scalar(
            "SELECT source_buzz_event_id FROM business_document_approval_votes WHERE request_id=$1",
        )
        .bind(
            result["requestId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .fetch_one(self.pool)
        .await
        .unwrap();
        assert_eq!(source, issued["sourceEventId"].as_str().unwrap());
        result
    }
    pub(super) async fn footprint(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('batches',(SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM operational_adjustment_batches b),'lines',(SELECT jsonb_agg(to_jsonb(l) ORDER BY id) FROM operational_adjustment_lines l),'votes',(SELECT count(*) FROM business_document_approval_votes),'requests',(SELECT count(*) FROM business_document_approval_requests),'facts',(SELECT count(*) FROM profit_facts),'numbering',(SELECT sum(current_value) FROM business_numbering_sequence_pools),'idem',(SELECT count(*) FROM business_command_idempotency),'outbox',(SELECT count(*) FROM business_core_outbox))").fetch_one(self.pool).await.unwrap()
    }
    pub(super) async fn verify(&self) -> i64 {
        for permission in ["profit_adjustment:update_draft", "profit_adjustment:read"] {
            sqlx::query("INSERT INTO business_role_permissions(role_id,permission_key) SELECT role_id,$2 FROM business_user_roles WHERE enterprise_user_id=$1 ON CONFLICT DO NOTHING").bind(self.fixture.actor).bind(permission).execute(self.pool).await.unwrap();
        }
        for action in ["profit_adjustment:create", "profit_adjustment:update_draft"] {
            sqlx::query("INSERT INTO business_approval_policies(action_code,required_permission,eligible_role_keys,min_approvers,allow_self_approval) VALUES($1,$1,ARRAY['b2_operator'],1,true)").bind(action).execute(self.pool).await.unwrap();
        }
        let create = "operational_adjustment_creation_intent";
        let update = "operational_adjustment_update_intent";
        for decision in ["approve", "reject"] {
            let before = self.footprint().await;
            let p = self.prepare(create, self.batch()).await;
            assert_eq!(
                self.footprint().await,
                before,
                "prepare must not save business data"
            );
            let result = self.approve(create, &p, decision).await;
            assert_eq!(result["executed"], decision == "approve");
            let id = if decision == "approve" {
                let id = result["createdDocument"]["id"]
                    .as_str()
                    .unwrap()
                    .parse::<Uuid>()
                    .unwrap();
                assert_eq!(batch_status(self.pool, id).await, "draft");
                let (found, _) = self
                    .call(
                        "search_operational_adjustments",
                        "profit_adjustment:read",
                        "查询刚创建的费用草稿",
                        json!({"number":result["createdDocument"]["number"]}),
                    )
                    .await;
                assert_eq!(found["status"], "ok", "{found}");
                assert_eq!(found["items"].as_array().unwrap().len(), 1);
                assert_eq!(found["items"][0]["id"], json!(id));
                let (detail, _) = self
                    .call(
                        "get_operational_adjustment",
                        "profit_adjustment:read",
                        "读取费用草稿完整明细",
                        json!({"documentId":id,"expectedVersion":1}),
                    )
                    .await;
                assert_eq!(detail["status"], "ok", "{detail}");
                assert_eq!(detail["items"][0]["version"], 1);
                assert_eq!(detail["items"][0]["lines"][0]["amount"], "10.010000");
                id
            } else {
                assert!(result["createdDocument"].is_null());
                assert_eq!(self.footprint().await["batches"], before["batches"]);
                fixture::draft(
                    self.pool,
                    self.fixture,
                    self.order,
                    "chain-rejected-update-source",
                )
                .await
            };
            let mut batch = self.batch();
            batch["lines"][0]["amount"] = json!("20.02");
            let before = self.footprint().await;
            let p = self
                .prepare(
                    update,
                    json!({"batchId":id,"expectedVersion":1,"batch":batch}),
                )
                .await;
            assert_eq!(self.footprint().await, before);
            let result = self.approve(update, &p, decision).await;
            assert_eq!(result["executed"], decision == "approve");
            let (version,amount):(i64,String)=sqlx::query_as("SELECT b.version,l.amount::text FROM operational_adjustment_batches b JOIN operational_adjustment_lines l ON l.batch_id=b.id WHERE b.id=$1").bind(id).fetch_one(self.pool).await.unwrap();
            assert_eq!(version, if decision == "approve" { 2 } else { 1 });
            assert_eq!(
                amount,
                if decision == "approve" {
                    "20.020000"
                } else {
                    "10.010000"
                }
            );
            assert_eq!(batch_status(self.pool, id).await, "draft");
            assert_eq!(self.footprint().await["facts"], before["facts"]);
        }
        for kind in [create, update] {
            for mode in ["revoked", "wrong_hash", "stale"] {
                let source = if kind == update {
                    Some(
                        fixture::draft(
                            self.pool,
                            self.fixture,
                            self.order,
                            &format!("draft-chain-{mode}"),
                        )
                        .await,
                    )
                } else {
                    None
                };
                let input = if let Some(id) = source {
                    json!({"batchId":id,"expectedVersion":1,"batch":self.batch()})
                } else {
                    self.batch()
                };
                let p = self.prepare(kind, input).await;
                let mut command = p["approvalCommand"].as_str().unwrap().to_string();
                if mode == "wrong_hash" {
                    command = command.replace(p["previewHash"].as_str().unwrap(), &"0".repeat(64));
                }
                let scope = format!("{kind}:approve");
                let issued =
                    issue(self.gateway, self.credential, self.keys, &command, &scope).await;
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
                        .header("x-trace-id", issued["traceId"].as_str().unwrap())
                        .send()
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::NO_CONTENT);
                }
                if mode == "stale" {
                    if let Some(id) = source {
                        sqlx::query("UPDATE operational_adjustment_batches SET version=version+1 WHERE id=$1").bind(id).execute(self.pool).await.unwrap();
                    } else {
                        sqlx::query("UPDATE sales_orders SET version=version+1 WHERE id=$1")
                            .bind(self.order)
                            .execute(self.pool)
                            .await
                            .unwrap();
                    }
                }
                let before = self.footprint().await;
                let mut client = mcp::Client::start(
                    self.binary,
                    self.gateway,
                    self.api,
                    self.credential,
                    &issued,
                    Some(&scope),
                )
                .await;
                let result = client
                    .call(
                        &format!("approve_{}", kind.strip_suffix("_intent").unwrap()),
                        json!({}),
                    )
                    .await;
                assert_ne!(result["executed"], true, "{mode}: {result}");
                assert_ne!(result["status"], "pending", "{mode}: {result}");
                client.stop().await;
                assert_eq!(self.footprint().await, before, "{kind} {mode}");
            }
        }
        for kind in [create, update] {
            let action = if kind == create {
                "profit_adjustment:create"
            } else {
                "profit_adjustment:update_draft"
            };
            sqlx::query(
                "UPDATE business_approval_policies SET min_approvers=2 WHERE action_code=$1",
            )
            .bind(action)
            .execute(self.pool)
            .await
            .unwrap();
            let input = if kind == create {
                self.batch()
            } else {
                let id = fixture::draft(
                    self.pool,
                    self.fixture,
                    self.order,
                    "draft-chain-pending-update",
                )
                .await;
                json!({"batchId":id,"expectedVersion":1,"batch":self.batch()})
            };
            let before = self.footprint().await;
            let p = self.prepare(kind, input).await;
            let result = self.approve(kind, &p, "approve").await;
            assert_eq!(result["status"], "pending");
            assert_eq!(result["executed"], false);
            assert_eq!(result["approvalCount"], 1);
            assert_eq!(result["minimumApprovers"], 2);
            let after = self.footprint().await;
            for key in ["batches", "lines", "facts", "numbering", "idem", "outbox"] {
                assert_eq!(after[key], before[key], "pending {key}");
            }
            sqlx::query(
                "UPDATE business_approval_policies SET min_approvers=1 WHERE action_code=$1",
            )
            .bind(action)
            .execute(self.pool)
            .await
            .unwrap();
        }
        // Twelve prepare/decision calls, two reads and six negative-case preparations.
        20
    }
}
