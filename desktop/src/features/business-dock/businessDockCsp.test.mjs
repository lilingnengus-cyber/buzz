import assert from "node:assert/strict";
import test from "node:test";

import {
  buildBusinessDockCsp,
  normalizeBusinessOrigin,
} from "../../../scripts/tauri-business-dock.mjs";

const baseCsp = "default-src 'self'; script-src 'self'; frame-src *";

test("business dock CSP adds only the exact configured origin", () => {
  const csp = buildBusinessDockCsp(baseCsp, "https://biz.example.com");
  assert.match(csp, /frame-src 'self' https:\/\/biz\.example\.com$/);
  assert.doesNotMatch(csp, /frame-src \*/);
});

test("business dock CSP stays self-only when the feature is unconfigured", () => {
  assert.match(buildBusinessDockCsp(baseCsp), /frame-src 'self'$/);
});

test("business dock CSP rejects non-origin and non-HTTP values", () => {
  for (const value of [
    "https://biz.example.com/embed/",
    "file:///tmp/business",
    "javascript:alert(1)",
  ]) {
    assert.throws(() => normalizeBusinessOrigin(value));
  }
});
