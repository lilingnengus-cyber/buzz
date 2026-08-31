import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(
  new URL("./AgentDefaultsEditor.tsx", import.meta.url),
  "utf8",
);

test("Agent defaults forces fresh harness discovery on open", () => {
  assert.match(source, /useAcpRuntimesQueryForced\(\)/);
  assert.doesNotMatch(source, /useAcpRuntimesQuery\(\)/);
});
