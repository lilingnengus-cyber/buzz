import assert from "node:assert/strict";
import test from "node:test";

import {
  lifeLinkAction,
  parseTrustedLifeExtensionResult,
} from "./lifeLinkHandler.ts";

const trace = "123e4567-e89b-42d3-a456-426614174000";
const audit = "123e4567-e89b-42d3-a456-426614174001";
const marker = [
  "pacioli-extension-result",
  "1",
  "life",
  "action.status.update",
  "succeeded",
  trace,
  audit,
];
const reference = [
  "pacioli-resource-ref",
  "1",
  trace,
  "life://action/action-1",
  "8",
  "接口设计",
];

test("ordinary click opens a validated Life link in Dock and modified click uses browser", () => {
  assert.equal(lifeLinkAction("life://action/action-1", false)?.action, "dock");
  assert.equal(
    lifeLinkAction("life://action/action-1", true)?.action,
    "browser",
  );
  assert.equal(lifeLinkAction("life://action/../escape", false), null);
});

test("trusted result tags preserve validated structured resource refs", () => {
  assert.deepEqual(parseTrustedLifeExtensionResult([marker, reference]), {
    operation: "action.status.update",
    traceId: trace,
    auditId: audit,
    resourceRefs: [
      {
        version: 1,
        extensionId: "life",
        type: "action",
        id: "action-1",
        path: "/embed/actions/action-1",
        title: "接口设计",
      },
    ],
  });
});

test("forged, ambiguous, and malformed result tags fail closed", () => {
  assert.equal(parseTrustedLifeExtensionResult([reference]), null);
  assert.equal(
    parseTrustedLifeExtensionResult([marker, marker, reference]),
    null,
  );
  assert.equal(
    parseTrustedLifeExtensionResult([
      marker,
      [...reference.slice(0, 2), audit, ...reference.slice(3)],
    ]),
    null,
  );
  assert.equal(
    parseTrustedLifeExtensionResult([
      marker,
      [...reference.slice(0, 3), "life://action/%252e%252e", "8", "x"],
    ]),
    null,
  );
  assert.equal(
    parseTrustedLifeExtensionResult([marker, [...reference, "extra"]]),
    null,
  );
});
