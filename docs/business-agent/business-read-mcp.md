# Business Read MCP

`services/business-read-mcp` uses the repository's `rmcp` stdio stack. It has no
network listener and registers 30 read tools, six fixed draft-creation tools,
and two zero-argument signed chat approval tools. The 14
authoritative Business Core reads are:

1. `get_sales_order`
2. `search_sales_orders`
3. `get_purchase_order`
4. `search_purchase_orders`
5. `query_inventory_balance`
6. `query_receivables`
7. `query_payables`
8. `query_order_profit`
9. `query_profitability_by_dimension`
10. `get_management_profit_report`
11. `get_management_report_snapshot`
12. `get_profit_evidence`
13. `get_operating_dashboard`
14. `get_business_data_quality`

The remaining read tools are eight deterministic anomaly reads, six
action-lifecycle reads, and two sales/purchase confirmation-preview reads. The
draft-write allowlist contains only sales order, shipment,
purchase order, goods receipt, customer receipt and supplier payment draft
creation. Draft tools cannot confirm, approve, execute, allocate, post, reverse
or edit.

The separate `approve_sales_order` and `approve_purchase_order` tools can only
consume an exact signed command already bound into the Delegation. They accept
no document parameters. The server uses the configured Business Core approval
policy threshold; when that threshold is one, one eligible approver invokes the
existing idempotent confirmation transaction.

There is no SQL, arbitrary URL/path, generic API, Shell, filesystem, browser, or
generic write tool. Every call follows:

```text
strict input validation
-> atomically consume scoped Delegation
-> Business Read API with service credential + validated context
-> bounded JSON/result-schema validation
-> allowlisted biz:// link validation
-> success/partial/failure audit
```

Draft writes additionally receive a server-generated idempotency key derived
from the delegation and tool name, then pass through Business Core's existing
user permission, data-scope, validation, idempotency and domain-audit checks.
They fail closed before consuming a delegation call when
`BUSINESS_AGENT_DRAFT_WRITE_ENABLED` is false; the default is false.

Default tool timeout is 10 seconds, result limit 100, payload limit 128 KiB.
The API client has no direct database access and does not forward Workbench,
Authentik, Embed, or Business Session credentials.

`BUSINESS_READ_ADAPTER=mock` is accepted only by debug builds and additionally
requires an explicit `Mock Only - Production Disabled` acknowledgement.
