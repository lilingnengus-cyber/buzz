use super::*;
use crate::b2::stock_reversal_guard::StockReversalGuard;

pub(super) async fn execute(
    state: &AppState,
    actor: Uuid,
    trace: Uuid,
    id: Uuid,
    kind: &str,
    input: &PrepareStockReversal,
    snapshot: &Value,
) -> Result<(), String> {
    let guard: StockReversalGuard =
        serde_json::from_value(snapshot["guard"].clone()).map_err(|e| e.to_string())?;
    let key = format!("agent-stock-reversal:{id}");
    let command = B2VersionCommand {
        expected_version: input.expected_source_version,
        reason_code: Some(input.reason.clone()),
    };
    match kind {
        "shipment_reversal_intent" => state
            .inventory
            .reverse_shipment_guarded(
                actor,
                trace,
                input.source_document_id,
                &key,
                &command,
                Some(&guard),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "inventory_opening_reversal_intent" => state
            .inventory
            .reverse_opening_guarded(
                actor,
                trace,
                input.source_document_id,
                &key,
                &command,
                Some(&guard),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "goods_receipt_reversal_intent" => state
            .receiving
            .reverse_receipt_guarded(
                actor,
                trace,
                input.source_document_id,
                &key,
                &B3VersionCommand {
                    expected_version: input.expected_source_version,
                    reason_code: Some(input.reason.clone()),
                },
                Some(&guard),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => Err("unknown stock reversal".into()),
    }
}
