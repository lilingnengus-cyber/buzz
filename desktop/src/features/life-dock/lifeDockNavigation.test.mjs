import assert from "node:assert/strict";
import test from "node:test";

import {
  canMoveLifeNavigation,
  canNavigateLifeResource,
  createLifeNavigationState,
  currentLifeNavigationEntry,
  moveLifeNavigation,
  pushLifeNavigation,
} from "./lifeDockNavigation.ts";

const resource = (id) => ({
  version: 1,
  extensionId: "life",
  type: "action",
  id,
  path: `/embed/actions/${id}`,
});

test("Life navigation truncates forward history after a new branch", () => {
  let state = createLifeNavigationState(resource("a"));
  state = pushLifeNavigation(state, resource("b"));
  state = pushLifeNavigation(state, resource("c"));
  state = moveLifeNavigation(state, -1);
  state = pushLifeNavigation(state, resource("d"));
  assert.deepEqual(
    state.entries.map((entry) => entry.id),
    ["a", "b", "d"],
  );
  assert.equal(currentLifeNavigationEntry(state)?.id, "d");
  assert.equal(canMoveLifeNavigation(state, 1), false);
  assert.equal(canMoveLifeNavigation(state, -1), true);
});

test("explicit navigation is allowed while automatic navigation respects safety state", () => {
  const base = {
    activeExtensionId: "life",
    dirty: false,
    followConversation: true,
    pinned: false,
  };
  assert.equal(
    canNavigateLifeResource({ ...base, source: "explicit", pinned: true }),
    true,
  );
  assert.equal(canNavigateLifeResource({ ...base, source: "automatic" }), true);
  assert.equal(
    canNavigateLifeResource({ ...base, source: "automatic", pinned: true }),
    false,
  );
  assert.equal(
    canNavigateLifeResource({ ...base, source: "automatic", dirty: true }),
    false,
  );
  assert.equal(
    canNavigateLifeResource({
      ...base,
      source: "automatic",
      followConversation: false,
    }),
    false,
  );
  assert.equal(
    canNavigateLifeResource({
      ...base,
      source: "automatic",
      activeExtensionId: "business",
    }),
    false,
  );
});
