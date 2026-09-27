import assert from "node:assert/strict";
import test from "node:test";
import {
  rememberRecentOperatingUnit,
  resolveRecentOperatingUnit,
} from "./recentOperatingUnit.ts";

const units = [
  { id: "east", status: "active" },
  { id: "west", status: "disabled" },
];

const storage = () => {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
};

test("remembers a valid operating unit independently for each workflow", () => {
  const memory = storage();
  rememberRecentOperatingUnit("sales-order", "east", memory);
  assert.equal(
    resolveRecentOperatingUnit("sales-order", units, "", memory),
    "east",
  );
  assert.equal(
    resolveRecentOperatingUnit("purchase-order", units, "fallback", memory),
    "fallback",
  );
});

test("ignores missing and inactive remembered operating units", () => {
  const memory = storage();
  rememberRecentOperatingUnit("sales-order", "west", memory);
  assert.equal(
    resolveRecentOperatingUnit("sales-order", units, "east", memory),
    "east",
  );
  rememberRecentOperatingUnit("sales-order", "removed", memory);
  assert.equal(
    resolveRecentOperatingUnit("sales-order", units, "east", memory),
    "east",
  );
});
