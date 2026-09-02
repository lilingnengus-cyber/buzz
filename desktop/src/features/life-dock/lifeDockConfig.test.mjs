import assert from "node:assert/strict";
import test from "node:test";

import { readLifeDockConfig } from "./lifeDockConfig.ts";

const validEnv = {
  LIFE_DOCK_ENABLED: "true",
  VITE_LIFE_APP_ORIGIN: "https://life.example.com",
  VITE_LIFE_APP_URL: "https://life.example.com/embed/",
};

test("life dock is disabled by default without requiring configuration", () => {
  assert.deepEqual(readLifeDockConfig({}), {
    enabled: false,
    config: null,
    error: null,
  });
});

test("life dock switch accepts only literal booleans", () => {
  assert.deepEqual(readLifeDockConfig({ LIFE_DOCK_ENABLED: "yes" }), {
    enabled: true,
    config: null,
    error: "LIFE_DOCK_ENABLED must be true or false.",
  });
});

test("enabled life dock requires both origin and home URL", () => {
  const result = readLifeDockConfig({ LIFE_DOCK_ENABLED: "true" });
  assert.equal(result.enabled, true);
  assert.equal(result.config, null);
  assert.match(result.error, /required/);
});

test("life dock accepts an HTTP(S) home URL on its exact origin", () => {
  assert.deepEqual(readLifeDockConfig(validEnv), {
    enabled: true,
    config: {
      homeUrl: "https://life.example.com/embed/",
      origin: "https://life.example.com",
    },
    error: null,
  });
});

test("renderer-safe Vite feature switch takes precedence", () => {
  assert.equal(
    readLifeDockConfig({
      ...validEnv,
      LIFE_DOCK_ENABLED: "false",
      VITE_LIFE_DOCK_ENABLED: "true",
    }).enabled,
    true,
  );
});

test("life dock rejects origins with paths, userinfo, query, or fragments", () => {
  for (const origin of [
    "file:///tmp/life",
    "https://user@life.example.com",
    "https://life.example.com/path",
    "https://life.example.com?query=1",
    "https://life.example.com#fragment",
    "https://life.example.com/",
  ]) {
    const result = readLifeDockConfig({
      ...validEnv,
      VITE_LIFE_APP_ORIGIN: origin,
    });
    assert.equal(result.config, null, origin);
    assert.match(result.error, /exact HTTP\(S\) origin/);
  }
});

test("life dock rejects cross-origin and credentialed home URLs", () => {
  for (const homeUrl of [
    "https://other.example.com/embed/",
    "https://user@life.example.com/embed/",
    "javascript:alert(1)",
  ]) {
    const result = readLifeDockConfig({
      ...validEnv,
      VITE_LIFE_APP_URL: homeUrl,
    });
    assert.equal(result.config, null, homeUrl);
  }
});
