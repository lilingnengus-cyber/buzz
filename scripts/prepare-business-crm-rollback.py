#!/usr/bin/env python3
"""Prepare a CRM-agent pause image source without reverting CRM security fixes."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("destination", type=Path)
parser.add_argument("--expected-router-sha256", required=True)
args = parser.parse_args()
source = args.source.resolve(strict=True)
destination = args.destination.resolve()
if destination.exists() or source == destination or source in destination.parents:
    raise SystemExit("destination must be a new directory outside the candidate source")
relative = Path("services/business-core/src/document_approval.rs")
original = (source / relative).read_bytes()
if hashlib.sha256(original).hexdigest() != args.expected_router_sha256:
    raise SystemExit("candidate router differs from reviewed source")
text = original.decode()
for module in ("crm",):
    old = f".merge({module}::routes())"
    if text.count(old) != 1:
        raise SystemExit(f"expected one unmodified route merge for {module}")
    text = text.replace(old, f".merge(paused_crm_routes({module}::routes()))")
text += '''
// Release rollback: pause CRM-agent operations while retaining scope fixes,
// current schema compatibility and human workbench CRM handling.
fn paused_crm_routes(routes: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    routes.route_layer(axum::middleware::from_fn(
        |_: axum::extract::Request, _: axum::middleware::Next| async {
            StatusCode::SERVICE_UNAVAILABLE
        },
    ))
}
'''
shutil.copytree(source, destination, symlinks=True)
(destination / relative).write_text(text)
manifest = {
    "mode": "pause-crm-agent-routes",
    "sourceRouterSha256": hashlib.sha256(original).hexdigest(),
    "pausedRouterSha256": hashlib.sha256(text.encode()).hexdigest(),
    "changedFiles": [str(relative)],
    "preserves": ["migrations", "CRM write authority and scope locks", "workbench CRM routes"],
}
(destination / "crm-rollback.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps(manifest))
