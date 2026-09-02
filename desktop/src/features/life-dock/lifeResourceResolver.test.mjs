import assert from "node:assert/strict";
import test from "node:test";

import { resolveLifeResource } from "./lifeResourceResolver.ts";

test("resolves every fixed life resource mapping", () => {
  const cases = [
    ["life://dashboard", "dashboard", undefined, "/embed/dashboard"],
    ["life://domain/domain-1", "domain", "domain-1", "/embed/domains/domain-1"],
    ["life://goal/goal_1", "goal", "goal_1", "/embed/goals/goal_1"],
    [
      "life://project/project.1",
      "project",
      "project.1",
      "/embed/projects/project.1",
    ],
    ["life://action/a-1", "action", "a-1", "/embed/actions/a-1"],
    [
      "life://calendar/2026-09-03",
      "calendar",
      "2026-09-03",
      "/embed/calendar?date=2026-09-03",
    ],
    ["life://journal/j-1", "journal", "j-1", "/embed/journal/j-1"],
    ["life://knowledge/k~1", "knowledge", "k~1", "/embed/knowledge/k~1"],
    ["life://review/r-1", "review", "r-1", "/embed/reviews/r-1"],
    [
      "life://ai-execution/run-1",
      "ai_execution",
      "run-1",
      "/embed/ai-executions/run-1",
    ],
    ["life://draft/d-1", "draft", "d-1", "/embed/drafts/d-1"],
  ];

  for (const [uri, type, id, path] of cases) {
    assert.deepEqual(resolveLifeResource(uri), {
      version: 1,
      extensionId: "life",
      type,
      ...(id ? { id } : {}),
      path,
    });
  }
});

test("rejects malformed, ambiguous, or executable resource links", () => {
  const tooLong = "a".repeat(129);
  for (const input of [
    "life://action",
    "life://dashboard/extra",
    "life://action/a/b",
    "life://action/..",
    "life://action/%2e%2e",
    "life://action/%252e%252e",
    "life://action/a%2Fb",
    "life://user@action/a-1",
    "life://action/a-1#fragment",
    "life://action/a-1?workspace=w-1",
    "life://action/a-1?token=secret",
    "life://action/a-1?email=user@example.com",
    "life://action/a-1?permission=write",
    "life://action/a-1?command=delete",
    "life://action/a-1?x=1&x=2",
    "life://unknown/a-1",
    `life://action/${tooLong}`,
    "life://calendar/2026-02-30",
    "life://calendar/2026-9-3",
    "https://life.example.com/action/a-1",
    "not a uri",
  ]) {
    assert.equal(resolveLifeResource(input), null, input);
  }
});

test("rejects non-string input and decodes a safe id exactly once", () => {
  assert.equal(resolveLifeResource({ type: "action", id: "a-1" }), null);
  assert.deepEqual(resolveLifeResource("life://action/action%2D1"), {
    version: 1,
    extensionId: "life",
    type: "action",
    id: "action-1",
    path: "/embed/actions/action-1",
  });
});
