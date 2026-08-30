import assert from "node:assert/strict";
import test from "node:test";
import { resolveBusinessEnvironmentLabel } from "./environmentLabel.ts";

test("labels the production Business hostname as Production", () => {
  assert.equal(
    resolveBusinessEnvironmentLabel(undefined, "business.shiyueshizi.com"),
    "Production",
  );
});

test("keeps non-production hosts on Staging by default", () => {
  assert.equal(
    resolveBusinessEnvironmentLabel(undefined, "127.0.0.1"),
    "Staging",
  );
});

test("uses the configured environment label when provided", () => {
  assert.equal(
    resolveBusinessEnvironmentLabel(" Acceptance ", "business.shiyueshizi.com"),
    "Acceptance",
  );
});
