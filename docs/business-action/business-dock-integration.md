# Business Dock integration

The resolver allowlists `biz://anomaly/<id>`, `biz://action-proposal/<id>`, `biz://work-item/<id>`, and `biz://approval-draft/<id>` and maps them to `/embed/anomalies/`, `/embed/action-proposals/`, `/embed/work-items/`, and `/embed/approval-drafts/`. IDs, origin, traversal, fragments, and query strings are strictly validated.

The acceptance fixture is labeled `Desensitized Acceptance UI` and `Production Disabled`. It shows finding detail, proposal detail, work-item preview, confirmed work item, and approval-draft detail. The approval page carries a fixed Draft Only warning.

Bridge V2 accepts only `work_item_created`, `work_item_status_changed`, `approval_draft_created`, `approval_draft_updated`, and `finding_acknowledged` action notifications. The Buzz host may toast, update the current resource, and refresh read queries. It does not publish to another channel, advance an Agent, approve, or call a business write.
