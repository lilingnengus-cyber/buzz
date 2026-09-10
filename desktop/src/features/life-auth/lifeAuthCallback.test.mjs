import assert from "node:assert/strict";
import test from "node:test";
import { normalizeLifeAuthCallback } from "./lifeAuthConfig.ts";
const config = { redirectUri: "https://life.shiyueshizi.com/auth/pacioli" };
test("normalizes only the exact Life desktop handoff to the registered redirect", () => {
  assert.equal(normalizeLifeAuthCallback("pacioli://auth/life-callback?code=a&state=b", config),
    "https://life.shiyueshizi.com/auth/pacioli?code=a&state=b");
  for (const value of ["pacioli://evil/life-callback?code=a", "pacioli://auth/business-callback?code=a", "pacioli://user@auth/life-callback?code=a", "pacioli://auth/life-callback?code=a#x"]) {
    assert.equal(normalizeLifeAuthCallback(value, config), value);
  }
  const old = { redirectUri: "pacioli://auth/life-callback" };
  assert.equal(normalizeLifeAuthCallback(old.redirectUri + "?state=a", old), old.redirectUri + "?state=a");
});
