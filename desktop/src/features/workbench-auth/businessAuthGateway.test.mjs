import assert from "node:assert/strict";
import test from "node:test";
import { readGatewayState } from "./businessAuthGateway.ts";

const pubkey = "a".repeat(64);
const issuer = "https://auth.example/application/o/workbench/";
const subject = "enterprise-user";

function setup(
  t,
  {
    bindings = [],
    userStatus = "active",
    mutateChallenge,
    mutateSignature,
    mutateBinding,
    onSign,
  } = {},
) {
  const calls = [];
  const signed = [];
  const issued = Math.floor(Date.now() / 1000);
  const expires = issued + 180;
  const challenge = {
    id: "challenge-id",
    audience: "bizfin-workbench-identity-binding",
    payload: `bizfin-identity-binding-v1\nchallenge_id=challenge-id\nnonce=opaque\naudience=bizfin-workbench-identity-binding\noidc_issuer=${issuer}\noidc_subject=${subject}\nbuzz_pubkey=${pubkey}\nissued_at=${issued}\nexpires_at=${expires}`,
    expiresAt: new Date(expires * 1000).toISOString(),
  };
  mutateChallenge?.(challenge);
  let current = true;
  t.mock.method(globalThis, "fetch", async (url, options) => {
    assert.equal(options.headers.Authorization, "Bearer session-token");
    calls.push({
      path: url.pathname,
      body: options.body && JSON.parse(options.body),
    });
    if (url.pathname === "/api/me")
      return Response.json({
        user: { id: subject, status: userStatus },
        workbenchSessionId: "session",
        bindings,
      });
    if (url.pathname.endsWith("/challenges")) return Response.json(challenge);
    if (url.pathname.endsWith("/verify"))
      return Response.json(
        mutateBinding?.({ pubkey }) ?? { buzzPubkey: pubkey, status: "active" },
      );
    throw new Error("Unexpected request");
  });
  const run = () =>
    readGatewayState("https://business.example", "session-token", {
      pubkey,
      issuer,
      subject,
      isCurrent: () => current,
      sign: async (input) => {
        signed.push(input);
        onSign?.(() => {
          current = false;
        });
        return (
          mutateSignature?.(input) ?? {
            ...input,
            pubkey,
            id: "signed-id",
            sig: "native-signature",
          }
        );
      },
    });
  return { calls, signed, run };
}

test("first login signs and verifies current chat identity using the authenticated account", async (t) => {
  const s = setup(t);
  assert.equal((await s.run()).status, "authenticated");
  assert.equal(s.calls.length, 3);
  assert.deepEqual(s.calls[1].body, { pubkey });
  assert.equal(s.signed[0].kind, 24243);
  assert.deepEqual(s.signed[0].tags, []);
  assert.equal(s.calls[2].body.signedEvent.sig, "native-signature");
});

test("existing active binding does not rotate or revoke delegations", async (t) => {
  const s = setup(t, { bindings: [{ buzzPubkey: pubkey, status: "active" }] });
  await s.run();
  assert.equal(s.calls.length, 1);
  assert.equal(s.signed.length, 0);
});

for (const options of [
  { bindings: [{ buzzPubkey: pubkey, status: "revoked" }] },
  { userStatus: "disabled" },
])
  test("revoked or disabled access cannot be restored by session refresh", async (t) => {
    const s = setup(t, options);
    await assert.rejects(s.run());
    assert.equal(s.calls.length, 1);
    assert.equal(s.signed.length, 0);
  });

for (const [name, mutateChallenge] of [
  [
    "subject",
    (c) => {
      c.payload = c.payload.replace(
        `oidc_subject=${subject}`,
        "oidc_subject=other",
      );
    },
  ],
  [
    "issuer",
    (c) => {
      c.payload = c.payload.replace(issuer, "https://other.example/");
    },
  ],
  [
    "pubkey",
    (c) => {
      c.payload = c.payload.replace(pubkey, "b".repeat(64));
    },
  ],
  [
    "expiry",
    (c) => {
      c.expiresAt = "2000-01-01T00:00:00Z";
    },
  ],
  [
    "audience",
    (c) => {
      c.audience = "other";
    },
  ],
])
  test(`refuses to sign a challenge with mismatched ${name}`, async (t) => {
    const s = setup(t, { mutateChallenge });
    await assert.rejects(s.run(), /challenge/);
    assert.equal(s.signed.length, 0);
    assert.equal(s.calls.length, 2);
  });

test("logout during native signing prevents verification", async (t) => {
  const s = setup(t, { onSign: (invalidate) => invalidate() });
  await assert.rejects(s.run(), /session changed/);
  assert.equal(s.calls.length, 2);
});

test("native identity switch prevents verification", async (t) => {
  const s = setup(t, {
    mutateSignature: (input) => ({ ...input, pubkey: "b".repeat(64) }),
  });
  await assert.rejects(s.run(), /identity changed/);
  assert.equal(s.calls.length, 2);
});

test("incorrect verify result is not reported as authenticated", async (t) => {
  const s = setup(t, {
    mutateBinding: () => ({ buzzPubkey: pubkey, status: "revoked" }),
  });
  await assert.rejects(s.run(), /not verified/);
});
