import assert from "node:assert/strict";
import test from "node:test";

import {
  buildChangeRequest,
  EMPTY_CHANGE_DRAFT,
} from "./changeRequestDraft.ts";

const catalog = {
  principals: [
    {
      id: "agent-id",
      kind: "independent_agent",
      externalId: "finance-agent",
      displayName: "Finance Agent",
      status: "active",
      version: 4,
      updatedAt: "2026-08-24T00:00:00Z",
      roles: [],
      permissions: [],
    },
  ],
  roles: [],
  permissions: [
    {
      id: "permission-id",
      capability: "inventory:read",
      resourceType: "inventory",
      action: "read",
      riskLevel: "low",
      status: "active",
      obligations: [],
      defaultDataScope: { mode: "unrestricted" },
      version: 1,
    },
  ],
};

test("builds a version-bound restricted grant without raw JSON input", () => {
  assert.deepEqual(
    buildChangeRequest(
      {
        ...EMPTY_CHANGE_DRAFT,
        principalId: "agent-id",
        capability: "inventory:read",
        scopeMode: "restricted",
        scopeDimension: "warehouse",
        scopeValues: "sh-01, sh-02, sh-01",
        obligations: ["human_approval"],
        reason: "Limit the digital employee to Shanghai warehouses",
      },
      catalog,
    ),
    {
      operation: "permission_grant",
      payload: {
        externalId: "finance-agent",
        capability: "inventory:read",
        dataScope: {
          mode: "restricted",
          dimensions: { warehouse: ["sh-01", "sh-02"] },
        },
        obligations: ["human_approval"],
        expectedVersion: 4,
      },
      reason: "Limit the digital employee to Shanghai warehouses",
    },
  );
});

test("requires a live catalog target", () => {
  assert.throws(
    () =>
      buildChangeRequest(
        { ...EMPTY_CHANGE_DRAFT, reason: "A valid administrative reason" },
        catalog,
      ),
    /Choose an active principal/,
  );
});
