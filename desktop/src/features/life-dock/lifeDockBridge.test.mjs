import assert from "node:assert/strict";
import test from "node:test";

import {
  createLifeBridgeMessage,
  parseInboundLifeBridgeMessage,
  readLifeBridgeEvent,
} from "./lifeDockBridge.ts";

const config = {
  homeUrl: "https://life.example.com/embed/dashboard",
  origin: "https://life.example.com",
};
const nonce = "life-session-nonce";
const source = {};

function event(data, overrides = {}) {
  return { data, origin: config.origin, source, ...overrides };
}

function v2(type, payload, overrides = {}) {
  return {
    version: 2,
    type,
    requestId: "req-1",
    sessionNonce: nonce,
    ...(payload === undefined ? {} : { payload }),
    ...overrides,
  };
}

function v3(type, payload, overrides = {}) {
  return {
    version: 3,
    type,
    requestId: "req-auth-1",
    sessionNonce: nonce,
    payload,
    ...overrides,
  };
}

test("accepts only the configured origin, source, and nonce", () => {
  assert.equal(
    readLifeBridgeEvent(event(v2("LIFE_READY")), source, config, nonce)?.type,
    "LIFE_READY",
  );
  assert.equal(
    readLifeBridgeEvent(
      event(v2("LIFE_READY"), { origin: "https://evil.example" }),
      source,
      config,
      nonce,
    ),
    null,
  );
  assert.equal(
    readLifeBridgeEvent(event(v2("LIFE_READY")), {}, config, nonce),
    null,
  );
  assert.equal(
    parseInboundLifeBridgeMessage(
      v2("LIFE_READY", undefined, { sessionNonce: "wrong" }),
      nonce,
    ),
    null,
  );
});

test("validates V2 navigation, resource, dirty, action, and data messages", () => {
  const resource = {
    version: 1,
    extensionId: "life",
    type: "action",
    id: "a-1",
    path: "/embed/actions/a-1",
  };
  assert.equal(
    parseInboundLifeBridgeMessage(
      v2("TITLE_CHANGED", { title: "Today" }),
      nonce,
    )?.type,
    "TITLE_CHANGED",
  );
  assert.equal(
    readLifeBridgeEvent(
      event(v2("ROUTE_CHANGED", { url: "/embed/actions/a-1" })),
      source,
      config,
      nonce,
    )?.payload.url,
    "https://life.example.com/embed/actions/a-1",
  );
  assert.deepEqual(
    parseInboundLifeBridgeMessage(v2("RESOURCE_CHANGED", { resource }), nonce)
      ?.payload,
    { resource },
  );
  assert.deepEqual(
    parseInboundLifeBridgeMessage(
      v2("DIRTY_STATE_CHANGED", { dirty: true }),
      nonce,
    )?.payload,
    { dirty: true },
  );
  assert.equal(
    parseInboundLifeBridgeMessage(
      v2("ACTION_COMPLETED", {
        action: "action_updated",
        message: "Action updated",
        resource,
        traceId: "trace-1",
      }),
      nonce,
    )?.type,
    "ACTION_COMPLETED",
  );
  assert.equal(
    parseInboundLifeBridgeMessage(
      v2("DATA_CHANGED", { resource, traceId: "trace-1" }),
      nonce,
    )?.type,
    "DATA_CHANGED",
  );
});

test("V3 carries only minimal authentication state", () => {
  assert.deepEqual(
    parseInboundLifeBridgeMessage(
      v3("AUTH_STATUS", { authenticated: true, user: { displayName: "Ada" } }),
      nonce,
    )?.payload,
    { authenticated: true, user: { displayName: "Ada" } },
  );
  assert.equal(
    parseInboundLifeBridgeMessage(
      v3("AUTH_REQUIRED", { reason: "Sign in required" }),
      nonce,
    )?.type,
    "AUTH_REQUIRED",
  );
  assert.equal(
    parseInboundLifeBridgeMessage(v3("SESSION_EXPIRED", {}), nonce)?.type,
    "SESSION_EXPIRED",
  );
  for (const leaked of [
    { token: "secret" },
    { cookie: "secret" },
    { workspaceId: "workspace-1" },
    { permissions: ["write"] },
    { email: "ada@example.com" },
  ]) {
    assert.equal(
      parseInboundLifeBridgeMessage(
        v3("AUTH_STATUS", {
          authenticated: true,
          user: { displayName: "Ada" },
          ...leaked,
        }),
        nonce,
      ),
      null,
    );
  }
});

test("rejects unknown versions, types, extra fields, and unsafe payloads", () => {
  const resource = {
    version: 1,
    extensionId: "life",
    type: "action",
    id: "a-1",
    path: "/embed/actions/a-1",
  };
  for (const message of [
    { ...v2("LIFE_READY"), version: 1 },
    v2("UNKNOWN", {}),
    { ...v2("LIFE_READY"), token: "secret" },
    v2("RESOURCE_CHANGED", {
      resource: { ...resource, path: "/embed/actions/other" },
    }),
    v2("ACTION_FAILED", { action: "delete_everything", message: "No" }),
    v2("ACTION_FAILED", {
      action: "action_updated",
      message: "No",
      stack: "private",
    }),
    v2("DIRTY_STATE_CHANGED", { dirty: "yes" }),
    v2("TITLE_CHANGED", { title: "x".repeat(181) }),
    v3("DATA_CHANGED", {}),
  ]) {
    assert.equal(parseInboundLifeBridgeMessage(message, nonce), null);
  }
  assert.equal(
    readLifeBridgeEvent(
      event(
        v2("ROUTE_CHANGED", { url: "https://evil.example/embed/actions/a-1" }),
      ),
      source,
      config,
      nonce,
    ),
    null,
  );
});

test("creates host messages with the required protocol version", () => {
  assert.deepEqual(
    createLifeBridgeMessage(
      "NAVIGATE",
      nonce,
      { path: "/embed/dashboard" },
      "req-nav",
    ),
    {
      version: 2,
      type: "NAVIGATE",
      requestId: "req-nav",
      sessionNonce: nonce,
      payload: { path: "/embed/dashboard" },
    },
  );
  assert.equal(
    createLifeBridgeMessage("CHECK_AUTH", nonce, undefined, "req-auth").version,
    3,
  );
});
