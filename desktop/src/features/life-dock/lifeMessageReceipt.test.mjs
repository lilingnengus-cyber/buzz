import assert from "node:assert/strict";
import test from "node:test";
import { lifeMessageReceipt } from "./lifeMessageReceipt.ts";
import { parseTrustedLifeExtensionResult } from "./lifeLinkHandler.ts";
const trace = "123e4567-e89b-42d3-a456-426614174000";
const audit = "123e4567-e89b-42d3-a456-426614174001";
const marker = [
  "pacioli-extension-result",
  "1",
  "life",
  "list_actions",
  "succeeded",
  trace,
  audit,
];
test("receipt contains identifiers once and preserves resource versions without listing unrelated titles", () => {
  const receipt = lifeMessageReceipt([
    marker,
    [
      "pacioli-resource-ref",
      "1",
      trace,
      "life://action/action-1",
      "8",
      "unrelated title",
    ],
  ]);
  assert.equal(
    receipt,
    `list_actions succeeded\nTrace ID: ${trace}\nAudit ID: ${audit}\nlife://action/action-1 v8`,
  );
});
test("failed receipts have no fabricated audit or navigable success result", () => {
  const failed = [...marker];
  failed[4] = "failed";
  failed[6] = "";
  assert.equal(
    lifeMessageReceipt([failed]),
    `list_actions failed\nTrace ID: ${trace}`,
  );
  assert.equal(parseTrustedLifeExtensionResult([failed]), null);
  assert.equal(lifeMessageReceipt([failed, failed]), null);
  failed[5] = "invalid";
  assert.equal(lifeMessageReceipt([failed]), null);
});
test("plain or malformed tags cannot supply a receipt", () => {
  assert.equal(lifeMessageReceipt([]), null);
  assert.equal(lifeMessageReceipt([marker, marker]), null);
  assert.equal(
    lifeMessageReceipt([
      marker,
      ["pacioli-resource-ref", "1", audit, "life://action/action-1", "1", ""],
    ]),
    null,
  );
});
