import assert from "node:assert/strict";
import test from "node:test";
import {
  createLifeWorkbenchSession,
  isLifeSessionRejected,
  LifeGatewayError,
} from "./lifeAuthGateway.ts";

test("temporary renewal failure can retry using the existing session without a nonce", async () => {
  const originalFetch = globalThis.fetch;
  const requests = [];
  globalThis.fetch = async (url, options) => {
    requests.push({
      path: new URL(url).pathname,
      body: JSON.parse(options.body),
    });
    return requests.length === 1
      ? new Response("{}", { status: 503 })
      : Response.json({
          sessionId: "session-1",
          sessionToken: "r".repeat(43),
          expiresAt: "2026-09-14T00:00:00Z",
        });
  };
  try {
    await assert.rejects(
      createLifeWorkbenchSession(
        "https://life.example.com",
        "header.e30.signature",
        null,
        "e".repeat(43),
      ),
      (error) => {
        assert.equal(isLifeSessionRejected(error), false);
        return true;
      },
    );
    const result = await createLifeWorkbenchSession(
      "https://life.example.com",
      "header.e30.signature",
      null,
      "e".repeat(43),
    );
    assert.equal(result.sessionToken, "r".repeat(43));
    assert.deepEqual(
      requests,
      Array(2).fill({
        path: "/v1/workbench/sessions/renew",
        body: { sessionToken: "e".repeat(43) },
      }),
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("only explicit auth rejection invalidates the renewal credential", () => {
  for (const status of [401, 403])
    assert.equal(
      isLifeSessionRejected(new LifeGatewayError("rejected", status)),
      true,
    );
  for (const status of [429, 500, 503])
    assert.equal(
      isLifeSessionRejected(new LifeGatewayError("temporary", status)),
      false,
    );
  assert.equal(isLifeSessionRejected(new TypeError("Failed to fetch")), false);
});

test("sleep recovery uses a fresh OIDC credential and a separate resume endpoint", async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async (url, options) => {
    assert.equal(new URL(url).pathname, "/v1/workbench/sessions/resume");
    assert.equal(options.headers.Authorization, "Bearer fresh-oidc");
    assert.deepEqual(JSON.parse(options.body), {
      sessionToken: "e".repeat(43),
    });
    return Response.json({
      sessionId: "restored",
      sessionToken: "r".repeat(43),
      expiresAt: "2026-09-22T00:00:00Z",
    });
  };
  try {
    await createLifeWorkbenchSession(
      "https://life.example.com",
      "fresh-oidc",
      null,
      "e".repeat(43),
      true,
    );
  } finally {
    globalThis.fetch = previous;
  }
});

test("a fresh interactive login can replace an obsolete saved session", async () => {
  const original = globalThis.fetch;
  const paths = [];
  globalThis.fetch = async (url) => {
    paths.push(new URL(url).pathname);
    return paths.length === 1
      ? Response.json({ error: "unauthorized" }, { status: 401 })
      : Response.json({
          sessionId: "new",
          sessionToken: "r".repeat(43),
          expiresAt: "2026-09-22T00:00:00Z",
        });
  };
  try {
    const token =
      "header." +
      Buffer.from(JSON.stringify({ nonce: "new-login" })).toString(
        "base64url",
      ) +
      ".signature";
    await createLifeWorkbenchSession(
      "https://life.example.com",
      token,
      null,
      "e".repeat(43),
      true,
    );
    assert.deepEqual(paths, [
      "/v1/workbench/sessions/resume",
      "/v1/workbench/sessions",
    ]);
  } finally {
    globalThis.fetch = original;
  }
});
