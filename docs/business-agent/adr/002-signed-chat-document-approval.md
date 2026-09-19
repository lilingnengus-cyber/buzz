# ADR 002: Signed chat approval for orders and inventory fulfillment

Status: accepted for the first production canary.

Business document approval is separate from V6 Business Action Approval Drafts. Approval Drafts remain non-executable. This stage permits sales-order, purchase-order, shipment and goods-receipt confirmation, plus opening-inventory posting, after the configured approval policy is satisfied.

The approval intent must be a complete signed Buzz message using one server-generated command:

```text
确认 sales-order <uuid> v<version> <preview-hash>
拒绝 purchase-order <uuid> v<version> <preview-hash>
```

The Gateway independently parses the signed event and binds document type, id, version, preview hash, and decision into the short-lived Delegation. The MCP approval tools take no arguments, so the model cannot substitute another document or decision. Legacy exact `/approve` and `/reject` commands remain supported. Signed confirmation events must be within five minutes of server time. Plain contextual confirmation, mentions, quoted commands, trailing text, malformed hashes, and unsupported document types do not receive approval scope.

Chat approval uses the underlying Business Core approval policy threshold; a
threshold of one permits one eligible approver to confirm the document. Policy
role eligibility, required permission, self-approval, distinct-business-unit
rule, current data scope, current document version, and current preview hash are
re-evaluated server-side. Votes and source Buzz event ids are unique and
append-only. Rejection stops the request. Reaching the threshold runs the
existing idempotent Business Core confirmation transaction.

This stage does not approve customer receipts, supplier payments, returns, adjustments, bank payments, or general-ledger posting. Policies requiring step-up authentication fail closed because this chat flow cannot supply that credential.

Inventory and cost changes produce a different preview hash; a previously failed confirmation can then be submitted as a new request even if the document version is unchanged. Repeating a completed confirmation cannot post twice. Automatic recovery of an execution failure with an unchanged preview is not supported.
