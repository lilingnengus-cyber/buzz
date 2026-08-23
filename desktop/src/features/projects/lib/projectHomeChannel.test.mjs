import assert from "node:assert/strict";
import test from "node:test";

import {
  findProjectHomeByChannelId,
  isProjectHomeChannel,
} from "./projectHomeChannel.ts";

test("isProjectHomeChannel is true when a project points at the channel", () => {
  assert.equal(
    isProjectHomeChannel("channel-a", [
      { projectChannelId: "channel-a" },
      { projectChannelId: "channel-b" },
    ]),
    true,
  );
});

test("isProjectHomeChannel is false for unbound channels", () => {
  assert.equal(
    isProjectHomeChannel("channel-z", [{ projectChannelId: "channel-a" }]),
    false,
  );
  assert.equal(
    isProjectHomeChannel(null, [{ projectChannelId: "channel-a" }]),
    false,
  );
});

test("findProjectHomeByChannelId prefers the oldest listed home", () => {
  const base = {
    createdAt: 0,
    legacy: false,
    projectChannelId: "channel-a",
    visibility: "listed",
  };
  const selected = findProjectHomeByChannelId("channel-a", [
    { ...base, createdAt: 200, id: "later" },
    { ...base, createdAt: 50, id: "hidden", visibility: "unlisted" },
    { ...base, createdAt: 100, id: "original" },
  ]);
  assert.equal(selected?.id, "original");
});
