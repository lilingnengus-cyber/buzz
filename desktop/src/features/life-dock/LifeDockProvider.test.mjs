import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { BUSINESS_DOCK_PREFERENCES_KEY } from "../business-dock/businessDockPreferences.ts";
import { canAttemptLifeRecovery } from "./lifeEmbedSession.ts";
import {
  DEFAULT_LIFE_DOCK_PREFERENCES,
  LIFE_DOCK_PREFERENCES_KEY,
  readLifeDockPreferences,
  saveLifeDockPreferences,
} from "./lifeDockPreferences.ts";
import {
  createInitialLifeDockState,
  lifeDockReducer,
} from "./lifeDockState.ts";

const config = {
  origin: "https://life.example.com",
  homeUrl: "https://life.example.com/embed/",
};

test("Life Dock state supports open, pin, follow, fullscreen, and dirty independently", () => {
  let state = createInitialLifeDockState(config, DEFAULT_LIFE_DOCK_PREFERENCES);
  state = lifeDockReducer(state, { type: "open" });
  state = lifeDockReducer(state, { type: "toggle-pinned" });
  state = lifeDockReducer(state, { type: "toggle-follow" });
  state = lifeDockReducer(state, { type: "toggle-fullscreen" });
  state = lifeDockReducer(state, { type: "dirty", dirty: true });
  assert.deepEqual(
    {
      open: state.open,
      pinned: state.pinned,
      follow: state.followConversation,
      fullscreen: state.fullscreen,
      dirty: state.dirty,
    },
    { open: true, pinned: true, follow: false, fullscreen: true, dirty: true },
  );
  assert.notEqual(LIFE_DOCK_PREFERENCES_KEY, BUSINESS_DOCK_PREFERENCES_KEY);
});

test("Life Dock preferences use a private storage record", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  saveLifeDockPreferences(storage, { followConversation: false, pinned: true });
  assert.deepEqual(readLifeDockPreferences(storage), {
    followConversation: false,
    pinned: true,
  });
  assert.equal(values.has(BUSINESS_DOCK_PREFERENCES_KEY), false);
});

test("automatic recovery is limited to one attempt", () => {
  assert.equal(canAttemptLifeRecovery(0), true);
  assert.equal(canAttemptLifeRecovery(1), false);
  assert.equal(canAttemptLifeRecovery(2), false);
});

test("Life iframe remains in the mounted Dock tree while visibility changes", () => {
  const source = readFileSync(
    new URL("./LifeDock.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /<LifeDockBrowser\s*\/>/u);
  assert.doesNotMatch(source, /state\.open\s*\?\s*<LifeDockBrowser/u);
  assert.match(
    source,
    /state\.open && active \? "visible" : "invisible pointer-events-none"/u,
  );
});
