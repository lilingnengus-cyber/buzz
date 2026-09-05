import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { BUSINESS_DOCK_PREFERENCES_KEY } from "../business-dock/businessDockPreferences.ts";
import { readBindingIssuedAt, readOidcNonce } from "./lifeAuthGateway.ts";
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

test("Life gateway reads the session nonce from an OIDC ID token", () => {
  const payload = Buffer.from(
    JSON.stringify({ nonce: "login-nonce" }),
  ).toString("base64url");
  assert.equal(readOidcNonce(`header.${payload}.signature`), "login-nonce");
  assert.equal(readOidcNonce("header.e30.signature"), null);
  assert.equal(readOidcNonce("not-a-token"), null);
});

test("Life binding uses the gateway challenge timestamp", () => {
  assert.equal(
    readBindingIssuedAt(
      "challenge_id=id\nissued_at=1788541815\nexpires_at=1788541905",
    ),
    1788541815,
  );
  assert.equal(readBindingIssuedAt("issued_at=1\nissued_at=2"), null);
  assert.equal(readBindingIssuedAt("issued_at=-1"), null);
});

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

test("manual Life OIDC reconnect releases the session-start lock and resumes after callback", () => {
  const source = readFileSync(
    new URL("./LifeDockProvider.tsx", import.meta.url),
    "utf8",
  );

  assert.match(
    source,
    /pendingOidcResumeRef\.current = true;\s*void lifeAuth\.signIn\(\);/u,
  );
  assert.doesNotMatch(source, /await lifeAuth\.signIn\(\)/u);
  assert.match(
    source,
    /lifeAuth\.phase !== "authenticated"[\s\S]*!pendingOidcResumeRef\.current[\s\S]*pendingOidcResumeRef\.current = false;[\s\S]*startLifeSession\(true\);/u,
  );
});

test("background resource synchronization never opens the Life Dock", () => {
  const resource = {
    version: 1,
    extensionId: "life",
    type: "action",
    id: "synced",
    path: "/embed/actions/synced",
  };
  const state = lifeDockReducer(
    createInitialLifeDockState(config, DEFAULT_LIFE_DOCK_PREFERENCES),
    {
      type: "sync-resource",
      url: "https://life.example.com/embed/actions/synced",
      resource,
    },
  );
  assert.equal(state.open, false);
  assert.equal(state.loading, false);
  assert.deepEqual(state.currentResource, resource);
});

test("Life iframe mounts on first open and remains mounted after close", () => {
  let state = createInitialLifeDockState(config, DEFAULT_LIFE_DOCK_PREFERENCES);
  assert.equal(state.browserMounted, false);
  assert.equal(state.frameUrl, "about:blank");
  state = lifeDockReducer(state, { type: "open" });
  assert.equal(state.browserMounted, true);
  state = lifeDockReducer(state, { type: "close" });
  assert.equal(state.browserMounted, true);

  const source = readFileSync(
    new URL("./LifeDock.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /state\.browserMounted \? <LifeDockBrowser \/>/u);
  assert.doesNotMatch(source, /state\.open\s*\?\s*<LifeDockBrowser/u);
  assert.match(
    source,
    /state\.open && active \? "visible" : "invisible pointer-events-none"/u,
  );
  assert.match(
    source,
    /state\.open && active && !floating && "border-l border-border\/70"/u,
  );
});
