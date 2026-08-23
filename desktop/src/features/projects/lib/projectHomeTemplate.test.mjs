import assert from "node:assert/strict";
import { test } from "node:test";

import {
  PROJECT_HOME_CHANNEL_TEMPLATE,
  PROJECT_HOME_TEMPLATE_ID,
  renderProjectHomeCanvas,
} from "./projectHomeTemplate.ts";

test("project home is the built-in default project template", () => {
  assert.equal(PROJECT_HOME_CHANNEL_TEMPLATE.id, PROJECT_HOME_TEMPLATE_ID);
  assert.equal(PROJECT_HOME_CHANNEL_TEMPLATE.isBuiltin, true);
  assert.equal(PROJECT_HOME_CHANNEL_TEMPLATE.name, "Project home");
});

test("project home canvas fills project, repository, and channel values", () => {
  const content = renderProjectHomeCanvas({
    channelId: "11111111-1111-4111-8111-111111111111",
    project: {
      id: "30621:owner:space-invaders",
      dtag: "space-invaders",
      name: "Space Invaders",
      owner: "a".repeat(64),
      repositories: [
        {
          cloneUrls: ["https://relay.example/git/owner/space-invaders"],
          dtag: "space-invaders",
          owner: "b".repeat(64),
        },
      ],
    },
  });

  assert.match(content, /# Project Channel: Space Invaders/);
  assert.match(content, /`space-invaders`/);
  assert.match(content, /b{64}/);
  assert.match(content, /https:\/\/relay\.example\/git\/owner\/space-invaders/);
  assert.match(content, /11111111-1111-4111-8111-111111111111/);
  assert.equal(content.includes("{{"), false);
  assert.match(content, /buzz issues status --issue <id>/);
  assert.match(content, /buzz pr open --repo-owner/);
  assert.match(content, /buzz canvas set .* --content -/);
});
