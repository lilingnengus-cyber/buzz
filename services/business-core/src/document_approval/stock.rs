use super::*;
use serde_json::Value;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/v1/agent-approval-previews/stock/{kind}/{id}",
            get(preview),
        )
        .route("/v1/agent-approvals/stock/{kind}/{id}", post(approve))
}

fn action(kind: &str) -> Option<&'static str> {
    match kind {
        "shipment" => Some("shipment:confirm"),
        "goods_receipt" => Some("goods_receipt:confirm"),
        "inventory_opening" => Some("inventory_opening:post"),
        _ => None,
    }
}

/// Load authority fields from a fixed document family, never from model-supplied SQL.
pub(super) async fn authority_row(
    store: &PgStore,
    kind: &str,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, StoreError> {
    let sql = match kind {
        "shipment" => "SELECT s.created_by_user_id,s.legal_entity_id,o.business_unit_id,o.customer_id party_id,s.warehouse_id,s.status lifecycle_status,s.version FROM shipments s JOIN sales_orders o ON o.id=s.sales_order_id WHERE s.id=$1",
        "goods_receipt" => "SELECT s.created_by_user_id,s.legal_entity_id,o.business_unit_id,o.supplier_id party_id,s.warehouse_id,s.status lifecycle_status,s.version FROM goods_receipts s JOIN purchase_orders o ON o.id=s.purchase_order_id WHERE s.id=$1",
        "inventory_opening" => "SELECT created_by_user_id,legal_entity_id,NULL::uuid business_unit_id,NULL::uuid party_id,NULL::uuid warehouse_id,status lifecycle_status,version FROM inventory_opening_batches WHERE id=$1",
        _ => return Err(StoreError::NotFoundOrForbidden),
    };
    sqlx::query(sql)
        .bind(id)
        .fetch_optional(store.pool())
        .await?
        .ok_or(StoreError::NotFoundOrForbidden)
}

pub(super) async fn check_stock_scope(
    store: &PgStore,
    actor: Uuid,
    kind: &str,
    id: Uuid,
) -> Result<(), StoreError> {
    let row = authority_row(store, kind, id).await?;
    let snapshot = store.snapshot(actor).await?;
    let read = match kind {
        "shipment" => "sales_order:read",
        "goods_receipt" => "goods_receipt:read",
        _ => "inventory:read",
    };
    if !snapshot.permission_keys.contains(read)
        || !snapshot
            .scopes
            .legal_entity_ids
            .contains(&row.get("legal_entity_id"))
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    if let Some(unit) = row.get::<Option<Uuid>, _>("business_unit_id") {
        if !snapshot.scopes.business_unit_ids.contains(&unit) {
            return Err(StoreError::NotFoundOrForbidden);
        }
    }
    if let Some(party) = row.get::<Option<Uuid>, _>("party_id") {
        let allowed = if kind == "shipment" {
            &snapshot.scopes.customer_ids
        } else {
            &snapshot.scopes.supplier_ids
        };
        if !allowed.contains(&party) {
            return Err(StoreError::NotFoundOrForbidden);
        }
    }
    let warehouses = if kind == "inventory_opening" {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT DISTINCT warehouse_id FROM inventory_opening_lines WHERE batch_id=$1",
        )
        .bind(id)
        .fetch_all(store.pool())
        .await?
    } else {
        row.get::<Option<Uuid>, _>("warehouse_id")
            .into_iter()
            .collect()
    };
    if warehouses.is_empty()
        || warehouses
            .iter()
            .any(|id| !snapshot.scopes.warehouse_ids.contains(id))
    {
        return Err(StoreError::NotFoundOrForbidden);
    }
    Ok(())
}

async fn value(state: &AppState, actor: Uuid, kind: &str, id: Uuid) -> Result<Value, StoreError> {
    check_stock_scope(&state.store, actor, kind, id).await?;
    match kind {
        "shipment" => serde_json::to_value(
            state
                .sales
                .shipment_confirmation_preview(actor, id)
                .await
                .map_err(|_| StoreError::NotFoundOrForbidden)?,
        )
        .map_err(|_| StoreError::Invalid("preview serialization".into())),
        "goods_receipt" => serde_json::to_value(
            state
                .receiving
                .confirmation_preview(actor, id)
                .await
                .map_err(|_| StoreError::NotFoundOrForbidden)?,
        )
        .map_err(|_| StoreError::Invalid("preview serialization".into())),
        "inventory_opening" => {
            let item: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'number',batch_number,'legalEntityId',legal_entity_id,'businessDate',business_date,'currency',currency,'status',status,'version',version) FROM inventory_opening_batches WHERE id=$1").bind(id).fetch_one(state.store.pool()).await?;
            let lines: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('warehouseId',l.warehouse_id,'warehouseName',w.name,'skuId',l.sku_id,'skuName',s.name,'quantity',l.quantity::text,'unitCost',l.unit_cost::text,'totalCost',l.total_cost::text,'ready',(e.status='active' AND bu.status='active' AND w.status='active' AND s.status='active' AND p.status='active' AND u.status='active' AND c.status='active' AND bu.legal_entity_id=e.id AND w.legal_entity_id=b.legal_entity_id AND (p.brand_id IS NULL OR EXISTS(SELECT 1 FROM business_brands br WHERE br.id=p.brand_id AND br.status='active')) AND (l.unit_cost<>0 OR p.allow_zero_cost))) FROM inventory_opening_lines l JOIN inventory_opening_batches b ON b.id=l.batch_id JOIN business_warehouses w ON w.id=l.warehouse_id JOIN business_legal_entities e ON e.id=w.legal_entity_id JOIN business_units bu ON bu.id=w.business_unit_id JOIN business_skus s ON s.id=l.sku_id JOIN business_products p ON p.id=s.product_id JOIN business_units_of_measure u ON u.id=p.base_uom_id JOIN business_product_categories c ON c.id=p.category_id WHERE l.batch_id=$1 ORDER BY l.line_number").bind(id).fetch_all(state.store.pool()).await?;
            let mut item = item;
            let ready = item["status"] == "draft"
                && !lines.is_empty()
                && lines.iter().all(|line| line["ready"] == true);
            item["canConfirm"] = json!(ready);
            item["readiness"] = json!(if ready {
                "ready"
            } else {
                "not_draft_or_master_data_not_ready"
            });
            item["lines"] = json!(lines);
            item["effect"] = json!("登记期初库存数量与成本，不产生采购应付或付款");
            Ok(item)
        }
        _ => Err(StoreError::NotFoundOrForbidden),
    }
}

async fn preview(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Response {
    match value(&state, c.actor_user_id, &kind, id).await {
        Ok(item) => {
            let row = match authority_row(&state.store, &kind, id).await {
                Ok(row) => row,
                Err(e) => return store_error(e, c.trace_id),
            };
            let document = json!({"legalEntityId":row.get::<Uuid,_>("legal_entity_id"),"warehouseId":row.get::<Option<Uuid>,_>("warehouse_id"),"businessUnitId":row.get::<Option<Uuid>,_>("business_unit_id"),"customerId":if kind=="shipment" {row.get::<Option<Uuid>,_>("party_id")}else{None},"supplierId":if kind=="goods_receipt" {row.get::<Option<Uuid>,_>("party_id")}else{None},"lines":item["lines"]});
            let hash = hash_json(&item);
            Json(json!({"item":item,"document":document,"previewHash":hash,"approvalCommand":format!("确认 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"rejectionCommand":format!("拒绝 {} {id} v{} {hash}",kind.replace('_',"-"),item["version"]),"traceId":c.trace_id})).into_response()
        }
        Err(e) => store_error(e, c.trace_id),
    }
}

async fn approve(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path((kind, id)): Path<(String, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ChatApprovalInput>,
) -> Response {
    let Some(action) = action(&kind) else {
        return approval_error(StatusCode::NOT_FOUND, "not_found_or_forbidden", c.trace_id);
    };
    let item = match value(&state, c.actor_user_id, &kind, id).await {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    if item["version"].as_i64() != Some(input.expected_version)
        || hash_json(&item) != input.preview_hash
    {
        return approval_error(StatusCode::CONFLICT, "stale_approval_preview", c.trace_id);
    }
    let Some(key) = idempotency_key(&headers) else {
        return approval_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            c.trace_id,
        );
    };
    let outcome = match cast_vote(
        &state.store,
        &kind,
        action,
        id,
        c.actor_user_id,
        c.trace_id,
        &input,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return store_error(e, c.trace_id),
    };
    let mut executed = false;
    let mut status = outcome.status.clone();
    if outcome.should_execute {
        let version = B2VersionCommand {
            expected_version: input.expected_version,
            reason_code: None,
        };
        let result = match kind.as_str() {
            "shipment" => state
                .inventory
                .confirm_shipment(c.actor_user_id, c.trace_id, id, key, &version)
                .await
                .map(|_| ()),
            "inventory_opening" => state
                .inventory
                .post_opening(c.actor_user_id, c.trace_id, id, key, &version)
                .await
                .map(|_| ()),
            _ => state
                .receiving
                .confirm_receipt(
                    c.actor_user_id,
                    c.trace_id,
                    id,
                    key,
                    &B3VersionCommand {
                        expected_version: input.expected_version,
                        reason_code: None,
                    },
                )
                .await
                .map(|_| ()),
        };
        if result.is_err() {
            let _ = finish_execution(&state.store, outcome.request_id, false).await;
            return approval_error(
                StatusCode::CONFLICT,
                "approval_execution_failed",
                c.trace_id,
            );
        }
        finish_execution(&state.store, outcome.request_id, true)
            .await
            .map_err(|e| store_error(e, c.trace_id))
            .ok();
        executed = true;
        status = "executed".into();
    }
    Json(json!({"requestId":outcome.request_id,"documentType":kind,"documentId":id,"decision":input.decision,"status":status,"approvalCount":outcome.approval_count,"minimumApprovers":outcome.minimum_approvers,"executed":executed,"traceId":c.trace_id})).into_response()
}

pub(crate) async fn opening_detail(
    State(state): State<Arc<AppState>>,
    Extension(c): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> Response {
    match value(&state, c.actor_user_id, "inventory_opening", id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => store_error(e, c.trace_id),
    }
}
