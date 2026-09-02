import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  buildWorkspaceDockCsp,
  normalizeLifeOrigin,
} from "../../../scripts/tauri-business-dock.mjs";

const baseCsp = readFileSync(
  new URL("../../../src-tauri/tauri.conf.json", import.meta.url),
  "utf8",
);

test("workspace Dock CSP contains only self and exact configured origins", () => {
  const config = JSON.parse(baseCsp);
  const csp = buildWorkspaceDockCsp(
    config.app.security.csp,
    "https://business.example.com",
    "https://life.example.com",
  );
  assert.match(
    csp,
    /frame-src 'self' https:\/\/business\.example\.com https:\/\/life\.example\.com$/u,
  );
  assert.doesNotMatch(
    csp,
    /frame-src[^;]*(?:\*|\bhttps:?(?:\s|;|$)|\bhttp:?(?:\s|;|$))/u,
  );
});

test("Life origin rejects paths, credentials, wildcard, and non-HTTP protocols", () => {
  for (const value of [
    "https://life.example.com/embed",
    "https://user@life.example.com",
    "https://*.example.com",
    "file:///tmp/life",
    "javascript:alert(1)",
  ]) {
    assert.throws(() => normalizeLifeOrigin(value), undefined, value);
  }
});
