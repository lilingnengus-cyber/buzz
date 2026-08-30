import assert from "node:assert/strict";
import test from "node:test";
import {
  isAllowedBusinessHostProtocol,
  logoutBusinessSession,
  parseBusinessHostAuthMessage,
  readBusinessSession,
} from "./businessDockBridge.ts";

const valid = {
  version: 3,
  type: "CHECK_AUTH",
  requestId: "request-1",
  sessionNonce: "nonce-1",
  payload: { interactive: false },
};

test("accepts a bounded Business Bridge V3 auth request", () => {
  assert.deepEqual(parseBusinessHostAuthMessage(valid), valid);
});

test("rejects wrong versions, missing nonce, and unknown auth commands", () => {
  assert.equal(parseBusinessHostAuthMessage({ ...valid, version: 2 }), null);
  assert.equal(
    parseBusinessHostAuthMessage({ ...valid, sessionNonce: "" }),
    null,
  );
  assert.equal(parseBusinessHostAuthMessage({ ...valid, type: "TOKEN" }), null);
});

test("allows the packaged Tauri host but rejects unrelated schemes", () => {
  assert.equal(isAllowedBusinessHostProtocol("tauri:"), true);
  assert.equal(isAllowedBusinessHostProtocol("https:"), true);
  assert.equal(isAllowedBusinessHostProtocol("file:"), false);
  assert.equal(isAllowedBusinessHostProtocol("javascript:"), false);
});

test("reads the current Business session identity", async () => {
  const session = await readBusinessSession(
    async () =>
      new Response(
        JSON.stringify({
          authenticated: true,
          subject: "user-1",
          displayName: "张三",
        }),
        { status: 200 },
      ),
  );
  assert.deepEqual(session, {
    authenticated: true,
    subject: "user-1",
    displayName: "张三",
  });
});

test("treats an absent Business session as unauthenticated", async () => {
  const session = await readBusinessSession(
    async () => new Response(null, { status: 401 }),
  );
  assert.equal(session, null);
});

test("logs out with a freshly rotated CSRF token", async () => {
  const calls = [];
  const fetchMock = async (path, init) => {
    calls.push({ path, init });
    if (path === "/api/session")
      return new Response(JSON.stringify({ csrfToken: "fresh-csrf" }), {
        status: 200,
      });
    return new Response(null, { status: 204 });
  };
  await logoutBusinessSession(fetchMock);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].path, "/api/logout");
  assert.equal(calls[1].init.method, "POST");
  assert.equal(calls[1].init.credentials, "include");
  assert.equal(calls[1].init.headers["x-csrf-token"], "fresh-csrf");
});

test("treats an already absent Business session as logged out", async () => {
  let calls = 0;
  await logoutBusinessSession(async () => {
    calls += 1;
    return new Response(null, { status: 401 });
  });
  assert.equal(calls, 1);
});
