#!/usr/bin/env python3
"""Prepare a master-agent pause image source while preserving current write authority."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("destination", type=Path)
parser.add_argument("--expected-router-sha256", required=True)
parser.add_argument("--include-order-hold", action="store_true",
                    help="Also pause the paired sales order hold agent routes")
parser.add_argument("--include-adjustments-and-reports", action="store_true",
                    help="Also pause adjustment and management/operating snapshot agent routes")
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
modules = ("master", "order_hold") if args.include_order_hold else ("master",)
if args.include_adjustments_and_reports:
    modules += ("adjustment", "report_snapshot", "operating_snapshot")
for module in modules:
    old = f".merge({module}::routes())"
    if text.count(old) != 1:
        raise SystemExit(f"expected one unmodified route merge for {module}")
    text = text.replace(old, f".merge(paused_master_routes({module}::routes()))")
text += '''
// Release rollback: pause master-agent operations while retaining scope fixes,
// current schema compatibility and human workbench master handling.
fn paused_master_routes(routes: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
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
    "mode": "pause-business-candidate-agent-routes" if args.include_adjustments_and_reports else ("pause-master-and-order-hold-agent-routes" if args.include_order_hold else "pause-master-agent-routes"),
    "pausedModules": list(modules),
    "sourceRouterSha256": hashlib.sha256(original).hexdigest(),
    "pausedRouterSha256": hashlib.sha256(text.encode()).hexdigest(),
    "changedFiles": [str(relative)],
    "preserves": ["migrations", "master write authority and scope locks", "workbench master routes"],
}
(destination / "master-rollback.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps(manifest))
