use super::*;

pub(super) async fn execute(
    state: &AppState,
    actor: Uuid,
    trace: Uuid,
    id: Uuid,
    kind: &str,
    input: &PrepareReversal,
) -> Result<(), String> {
    let key = format!("agent-reversal:{id}");
    let reason = Some(input.reason.as_str());
    match kind {
        "customer_receipt_reversal_intent" => state
            .settlement
            .reverse_receipt_with_reason(
                actor,
                trace,
                input.source_document_id,
                &key,
                &crate::b2::model::VersionCommand {
                    expected_version: input.expected_source_version,
                    reason_code: Some(input.reason.clone()),
                },
                reason,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "supplier_payment_reversal_intent" => state
            .payables
            .reverse_payment_with_reason(
                actor,
                trace,
                input.source_document_id,
                &key,
                &crate::b3::model::VersionCommand {
                    expected_version: input.expected_source_version,
                    reason_code: Some(input.reason.clone()),
                },
                reason,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "receivable_allocation_reversal_intent" => state
            .settlement
            .reverse_allocation_with_reason(
                actor,
                trace,
                input.allocation_id.ok_or("allocation required")?,
                &key,
                &crate::b2::model::ReverseAllocation {
                    expected_receipt_version: input.expected_source_version,
                    expected_receivable_version: input
                        .expected_target_version
                        .ok_or("target version required")?,
                },
                reason,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "payable_allocation_reversal_intent" => state
            .payables
            .reverse_allocation_with_reason(
                actor,
                trace,
                input.allocation_id.ok_or("allocation required")?,
                &key,
                &crate::b3::model::ReversePayableAllocation {
                    expected_payment_version: input.expected_source_version,
                    expected_payable_version: input
                        .expected_target_version
                        .ok_or("target version required")?,
                },
                reason,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => Err("unknown reversal".into()),
    }
}
