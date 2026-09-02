import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

const source = fs.readFileSync(
  new URL("./AppExtensionProviders.tsx", import.meta.url),
  "utf8",
);

test("extension providers preserve auth and IAM order around the generic dock host", () => {
  const workbench = source.indexOf("<WorkbenchAuthProvider>");
  const gate = source.indexOf("<WorkbenchAuthGate>");
  const iam = source.indexOf("<BusinessIamAdminProvider>");
  const dockHost = source.indexOf("<WorkspaceDockHostProvider");
  assert.ok(workbench >= 0);
  assert.ok(workbench < gate);
  assert.ok(gate < iam);
  assert.ok(iam < dockHost);
  assert.match(
    source,
    /APP_WORKSPACE_DOCK_EXTENSIONS = \[\s*businessDockExtension,\s*\.\.\.\(lifeDockExtension \? \[lifeDockExtension\] : \[\]\),\s*\]/u,
  );
  assert.match(source, /extensions=\{APP_WORKSPACE_DOCK_EXTENSIONS\}/);
});
