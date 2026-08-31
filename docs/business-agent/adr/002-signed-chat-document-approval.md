# ADR 002: Signed chat approval for sales and purchase orders

Status: accepted for the first production canary.

Business document approval is separate from V6 Business Action Approval Drafts. Approval Drafts remain non-executable. This stage permits only sales-order and purchase-order confirmation after the configured approval policy is satisfied.

The approval intent must be a complete signed Buzz message using one server-generated command:

```text
/approve sales-order <uuid> v<version> <preview-hash>
/reject purchase-order <uuid> v<version> <preview-hash>
```

The Gateway independently parses the signed event and binds document type, id, version, preview hash, and decision into the short-lived Delegation. The MCP approval tools take no arguments, so the model cannot substitute another document or decision. Plain natural language, mentions, quoted commands, trailing text, malformed hashes, and unsupported document types do not receive approval scope.

Chat approval uses the underlying Business Core approval policy threshold; a
threshold of one permits one eligible approver to confirm the document. Policy
role eligibility, required permission, self-approval, distinct-business-unit
rule, current data scope, current document version, and current preview hash are
re-evaluated server-side. Votes and source Buzz event ids are unique and
append-only. Rejection stops the request. Reaching the threshold runs the
existing idempotent Business Core confirmation transaction.

This stage does not approve shipments, goods receipts, customer receipts, supplier payments, returns, adjustments, bank payments, or general-ledger posting. Those require separate risk acceptance.
