import assert from "node:assert/strict";
import test from "node:test";

import {
  buildLifeEmbedBootstrapUrl,
  canAttemptLifeRecovery,
  lifeSessionRenewalDelay,
  parseLifeEmbedCallback,
  readLifeEmbedCode,
  validateLifeEmbedUrl,
} from "./lifeEmbedSession.ts";

const config = {
  homeUrl: "https://life.example.com/embed/dashboard",
  origin: "https://life.example.com",
};

test("accepts only the exact Life callback and one 256-bit code", () => {
  const code = "a".repeat(43);
  assert.equal(
    parseLifeEmbedCallback(`pacioli://auth/life-bootstrap?code=${code}`),
    code,
  );
  for (const value of [
    `buzz://auth/life-bootstrap?code=${code}`,
    `pacioli://auth/business-bootstrap?code=${code}`,
    `pacioli://auth/life-bootstrap?code=${code}&token=secret`,
    `pacioli://auth/life-bootstrap?code=${code}&code=${code}`,
    `pacioli://user@auth/life-bootstrap?code=${code}`,
    `pacioli://auth/life-bootstrap?code=${code}#fragment`,
    "pacioli://auth/life-bootstrap?code=short",
  ]) {
    assert.equal(parseLifeEmbedCallback(value), null, value);
  }
});

test("reads a renewal code only from a validated Life bootstrap URL", () => {
  const code = "c".repeat(43);
  assert.equal(
    readLifeEmbedCode(
      config,
      `https://life.example.com/embed/bootstrap?code=${code}`,
    ),
    code,
  );
  assert.equal(
    readLifeEmbedCode(
      config,
      `https://evil.example/embed/bootstrap?code=${code}`,
    ),
    null,
  );
});

test("renews ninety seconds before the Workbench session expires", () => {
  const now = Date.parse("2026-09-05T00:00:00.000Z");
  assert.equal(
    lifeSessionRenewalDelay("2026-09-05T01:00:00.000Z", now),
    3_510_000,
  );
  assert.equal(lifeSessionRenewalDelay("2026-09-05T00:01:00.000Z", now), 0);
  assert.equal(lifeSessionRenewalDelay("invalid", now), null);
});

test("builds and validates bootstrap URLs only on the configured origin", () => {
  const code = "b".repeat(43);
  const expected = `https://life.example.com/embed/bootstrap?code=${code}`;
  assert.equal(buildLifeEmbedBootstrapUrl(config, code), expected);
  assert.equal(validateLifeEmbedUrl(config, expected), expected);
  assert.equal(
    validateLifeEmbedUrl(
      config,
      `https://evil.example/embed/bootstrap?code=${code}`,
    ),
    null,
  );
  assert.equal(buildLifeEmbedBootstrapUrl(config, "short"), null);
});

test("automatic recovery is capped at one attempt", () => {
  assert.equal(canAttemptLifeRecovery(0), true);
  assert.equal(canAttemptLifeRecovery(1), false);
  assert.equal(canAttemptLifeRecovery(2), false);
  assert.equal(canAttemptLifeRecovery(-1), false);
});
