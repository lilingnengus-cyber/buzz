import test from "node:test";
import assert from "node:assert/strict";
import { amountMinor, isDue, localDate } from "./crm.ts";
test("CRM currency input keeps cents exact and rejects invalid amounts", () => {
  assert.equal(amountMinor("123.45"), 12345);
  assert.equal(amountMinor("0"), 0);
  assert.equal(amountMinor(""), null);
  for (const value of ["-1", "1.001", "NaN", "1e9", "10000000000"])
    assert.throws(() => amountMinor(value));
});
test("due filter excludes closed opportunities and uses local calendar date", () => {
  assert.equal(localDate(new Date(2026, 8, 19, 0, 1)), "2026-09-19");
  assert.equal(
    isDue({ stage: "quoting", nextFollowUp: "2026-09-18" }, "2026-09-19"),
    true,
  );
  for (const stage of ["won", "lost"])
    assert.equal(
      isDue({ stage, nextFollowUp: "2026-09-18" }, "2026-09-19"),
      false,
    );
  assert.equal(
    isDue({ stage: "new", nextFollowUp: null }, "2026-09-19"),
    false,
  );
});
