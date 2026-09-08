import assert from "node:assert/strict";
import test from "node:test";

import { messageMentionPubkeys } from "./messageMentionPubkeys.ts";

function channel(overrides = {}) {
  return {
    id: "dm-1",
    name: "DM",
    channelType: "dm",
    visibility: "private",
    description: "",
    topic: null,
    purpose: null,
    memberCount: 2,
    memberPubkeys: ["OWNER", "AGENT"],
    participantPubkeys: ["owner", "agent"],
    participants: [],
    lastMessageAt: null,
    archivedAt: null,
    isMember: true,
    ttlSeconds: null,
    ttlDeadline: null,
    ...overrides,
  };
}

test("plain DM messages p-tag every recipient except the sender", () => {
  assert.deepEqual(messageMentionPubkeys(channel(), "owner"), ["agent"]);
});

test("DM recipients and explicit mentions are normalized and deduplicated", () => {
  assert.deepEqual(
    messageMentionPubkeys(channel(), "OWNER", ["AGENT", "third"]),
    ["agent", "third"],
  );
});

test("stream messages preserve explicit-mention semantics", () => {
  assert.deepEqual(
    messageMentionPubkeys(
      channel({ channelType: "stream", memberPubkeys: ["owner", "agent"] }),
      "owner",
      [],
    ),
    [],
  );
});

const { resolveMessageMentionPubkeys } = await import(
  "./messageMentionPubkeys.ts"
);

test("reconnect with an empty DM roster loads recipients before sending", async () => {
  const calls = [];
  const result = await resolveMessageMentionPubkeys(
    channel({ memberPubkeys: [], participantPubkeys: [] }),
    "owner",
    [],
    async (id) => {
      calls.push(id);
      return [{ pubkey: "owner" }, { pubkey: "agent" }];
    },
  );
  assert.deepEqual(calls, ["dm-1"]);
  assert.deepEqual(result, ["agent"]);
});

test("an explicit mention does not hide an incomplete DM roster", async () => {
  assert.deepEqual(
    await resolveMessageMentionPubkeys(
      channel({ memberPubkeys: ["owner"], participantPubkeys: [] }),
      "owner",
      ["third"],
      async () => [{ pubkey: "owner" }, { pubkey: "agent" }],
    ),
    ["third", "agent"],
  );
});

test("missing DM recipients and membership failures abort the send", async () => {
  const incomplete = channel({ memberPubkeys: [], participantPubkeys: [] });
  await assert.rejects(
    resolveMessageMentionPubkeys(incomplete, "owner", [], async () => [
      { pubkey: "owner" },
    ]),
    /收件人/,
  );
  await assert.rejects(
    resolveMessageMentionPubkeys(incomplete, "owner", [], async () => {
      throw new Error("disconnected");
    }),
    /disconnected/,
  );
});

test("complete DMs and streams do not require another membership request", async () => {
  const unexpected = async () => assert.fail("unexpected membership read");
  assert.deepEqual(
    await resolveMessageMentionPubkeys(channel(), "owner", [], unexpected),
    ["agent"],
  );
  assert.deepEqual(
    await resolveMessageMentionPubkeys(
      channel({ channelType: "stream" }),
      "owner",
      [],
      unexpected,
    ),
    [],
  );
});
