import assert from "node:assert/strict";
import test from "node:test";

import {
  createWorkspaceDockHostState,
  reportWorkspaceDockState,
  requestWorkspaceDockActivation,
} from "./workspaceDockStore.ts";

test("business and life dock state remains isolated", () => {
  const initial = createWorkspaceDockHostState(["business", "life"]);
  const next = reportWorkspaceDockState(initial, "business", {
    dirty: true,
    open: true,
  });
  assert.equal(next.docks.business?.dirty, true);
  assert.equal(next.docks.business?.open, true);
  assert.equal(next.docks.life?.dirty, false);
  assert.equal(next.docks.life?.open, false);
});

test("only one dock is active while inactive state stays mounted in the store", () => {
  const initial = createWorkspaceDockHostState(["business", "life"]);
  const business = requestWorkspaceDockActivation(initial, "business");
  assert.equal(business.allowed, true);
  assert.equal(business.state.docks.business?.active, true);
  assert.equal(business.state.docks.life?.active, false);

  const life = requestWorkspaceDockActivation(business.state, "life");
  assert.equal(life.allowed, true);
  assert.equal(life.state.docks.business?.active, false);
  assert.equal(life.state.docks.life?.active, true);
  assert.ok(life.state.docks.business);
});

test("dirty active dock blocks a switch and unknown ids fail closed", () => {
  let state = createWorkspaceDockHostState(["business", "life"]);
  state = requestWorkspaceDockActivation(state, "business").state;
  state = reportWorkspaceDockState(state, "business", { dirty: true });
  const blocked = requestWorkspaceDockActivation(state, "life");
  assert.equal(blocked.allowed, false);
  assert.equal(blocked.reason, "dirty-active-dock");
  assert.equal(blocked.state.activeExtensionId, "business");

  const unknown = requestWorkspaceDockActivation(state, "unknown");
  assert.equal(unknown.allowed, false);
  assert.equal(unknown.reason, "unknown-extension");
});
