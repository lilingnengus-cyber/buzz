import assert from "node:assert/strict";
import test from "node:test";

import { pickDefaultProjectsAgent } from "./projectAgentSelection.ts";

test("prefers Fizz over the first running agent", () => {
  const implementationPartner = {
    name: "Implementation Partner",
    personaId: "custom:implementation",
  };
  const fizz = { name: "Fizz", personaId: "builtin:fizz" };
  assert.equal(pickDefaultProjectsAgent([implementationPartner, fizz]), fizz);
});

test("keeps legacy Kit compatibility and falls back to first", () => {
  const first = { name: "Builder" };
  const kit = { name: "Kit" };
  assert.equal(pickDefaultProjectsAgent([first, kit]), kit);
  assert.equal(pickDefaultProjectsAgent([first]), first);
  assert.equal(pickDefaultProjectsAgent([]), null);
});
