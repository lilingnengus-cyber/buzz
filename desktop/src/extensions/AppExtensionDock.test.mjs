import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

const dockSource = fs.readFileSync(
  new URL("./AppExtensionDock.tsx", import.meta.url),
  "utf8",
);
const actionSource = fs.readFileSync(
  new URL("./AppExtensionTopChromeActions.tsx", import.meta.url),
  "utf8",
);
const adapterSource = fs.readFileSync(
  new URL(
    "../features/business-dock/businessDockExtension.tsx",
    import.meta.url,
  ),
  "utf8",
);

test("app dock and top chrome render through generic workspace slots", () => {
  assert.match(dockSource, /<WorkspaceDockHost\s*\/>/);
  assert.match(actionSource, /<BusinessIamAdminTopChromeAction\s*\/>/);
  assert.match(actionSource, /<WorkspaceDockTopChromeActions\s*\/>/);
});

test("business adapter preserves the existing provider, dock, action, and scheme", () => {
  assert.match(adapterSource, /id: "business"/);
  assert.match(adapterSource, /scheme: "biz"/);
  assert.match(adapterSource, /Provider: BusinessDockExtensionProvider/);
  assert.match(
    adapterSource,
    /<BusinessDockProvider>\{children\}<\/BusinessDockProvider>/,
  );
  assert.match(adapterSource, /Dock: BusinessDock/);
  assert.match(adapterSource, /TopChromeAction: BusinessDockTopChromeAction/);
  assert.match(adapterSource, /resolveBusinessResource/);
});
