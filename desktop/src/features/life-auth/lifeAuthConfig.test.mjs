import assert from "node:assert/strict";
import test from "node:test";

import { readLifeAuthConfig } from "./lifeAuthConfig.ts";

const valid = {
  VITE_LIFE_OIDC_ISSUER:
    "https://auth.example.test/application/o/life-workbench/",
  VITE_LIFE_OIDC_CLIENT_ID: "pacioli-life-workbench",
  VITE_LIFE_OIDC_REDIRECT_URI: "pacioli://auth/life-callback",
  VITE_LIFE_OIDC_POST_LOGOUT_REDIRECT_URI:
    "pacioli://auth/life-logout-callback",
};

test("Life OIDC has its own complete configuration", () => {
  const result = readLifeAuthConfig(valid);
  assert.equal(result.error, null);
  assert.equal(result.config?.clientId, "pacioli-life-workbench");
  assert.equal(result.config?.redirectUri, "pacioli://auth/life-callback");
});

test("Life OIDC reports its own environment variables", () => {
  const result = readLifeAuthConfig({
    VITE_LIFE_OIDC_CLIENT_ID: "partial",
  });
  assert.equal(result.config, null);
  assert.match(result.error, /Life OIDC/u);
  assert.match(result.error, /VITE_LIFE_OIDC_/u);
});
