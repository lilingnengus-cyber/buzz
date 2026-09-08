# Life concise replies — implementation and validation

Branch: `codex/life-concise-replies`.

New replies keep requested results in the body and move service identifiers and
resource versions into one collapsed, copyable execution-details section. The
message renderer requires an agent signer and validated structured tags. Failed
receipts remain failures and do not authorize automatic resource navigation.
High-risk confirmation and gateway/audit behavior are unchanged.

The agent prompt requires explicit parent-child relationships when answering
subtask queries; a flat resource-reference list is not a list of children. ACP
no longer appends that flat list to the answer. Read-reply cleanup removes legacy
receipt labels/appendices without applying that cleanup to authoritative write,
confirmation, failure, or channel-disclosure text.

## Validation

- Life response Rust tests: 12 passed on the final implementation.
- Desktop unit tests: 6,595 passed, including the new receipt parser tests.
- Life Dock Playwright workflow: 3 passed. The workflow checks a collapsed receipt,
  expand/collapse, exact clipboard contents, and existing Dock behavior.
- Rust formatting and ACP Clippy (all targets/features): passed.
- Business extension boundary check and diff whitespace check: passed.
- Live acceptance script: shell syntax and four receipt-predicate fixtures passed.
  The model-backed live workflow was not run; no LifeOS data was changed.
- Full repository gate reached Tauri tests, then ran out of local disk. Only
  task-created incremental compilation sessions were removed. The remaining gate
  completed successfully via `just desktop-tauri-test web-build mobile-test`:
  Tauri tests and Web build passed; all 2,074 mobile tests passed. Earlier passing
  gate results were retained, with focused Rust tests/Clippy rerun for the final
  receipt-label boundary adjustment.

## Delivery state

Implemented in the isolated worktree; not installed, deployed, or merged.
Install the matching Desktop renderer before updating the ACP publisher. Older
clients can read the answer but do not display the new tagged execution details.
Historical message bodies are not rewritten. Subtask selection remains an agent
instruction based on returned relationships, not a new server-side query filter.
