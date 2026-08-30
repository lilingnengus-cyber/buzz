import { expect, test, type Page, type Route } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

const catalog = {
  principals: [
    {
      id: "requester-id",
      kind: "human",
      externalId: "requester-user",
      displayName: "Lin Requester",
      status: "active",
      version: 2,
      updatedAt: "2026-08-24T08:00:00Z",
      roles: [{ code: "iam-admin", name: "IAM administrator" }],
      permissions: [],
    },
    {
      id: "agent-id",
      kind: "independent_agent",
      externalId: "finance-agent",
      displayName: "Finance Digital Employee",
      status: "active",
      version: 4,
      updatedAt: "2026-08-24T08:00:00Z",
      roles: [],
      permissions: [],
    },
  ],
  roles: [
    {
      id: "finance-role-id",
      code: "finance.operator",
      name: "Finance operator",
      status: "active",
      version: 3,
      updatedAt: "2026-08-24T08:00:00Z",
      permissions: [],
    },
  ],
  permissions: [
    {
      id: "sales-write-id",
      capability: "sales_order:write",
      resourceType: "sales_order",
      action: "write",
      riskLevel: "high",
      status: "active",
      obligations: ["human_approval", "step_up_authentication", "dual_control"],
      defaultDataScope: { mode: "unrestricted" },
      version: 1,
    },
  ],
};

function pendingChange() {
  return {
    id: "10000000-0000-4000-8000-000000000001",
    operation: "permission_grant",
    payload: {
      externalId: "finance-agent",
      capability: "sales_order:write",
      dataScope: {
        mode: "restricted",
        dimensions: { legal_entity: ["cn"] },
      },
      expectedVersion: 4,
    },
    riskLevel: "critical",
    requiredApprovals: 1,
    approvalCount: 0,
    status: "pending",
    requestedBy: "requester-id",
    requesterDisplayName: "Lin Requester",
    approvals: [],
    reason: "Allow controlled sales-order maintenance for the China entity",
    traceId: "20000000-0000-4000-8000-000000000002",
    requestedAt: "2026-08-24T08:00:00Z",
    expiresAt: "2026-08-25T08:00:00Z",
    decidedAt: null,
    appliedAt: null,
    failureCode: null,
    version: 1,
  };
}

async function fulfillJson(route: Route, body: unknown, status = 200) {
  await route.fulfill({
    status,
    contentType: "application/json",
    headers: {
      "access-control-allow-origin": "http://127.0.0.1:4173",
      "access-control-allow-headers": "authorization,content-type",
      "access-control-allow-methods": "GET,POST,OPTIONS",
    },
    body: JSON.stringify(body),
  });
}

async function installIamApi(page: Page) {
  let change = pendingChange();
  await page.route("http://127.0.0.1:3110/**", async (route) => {
    const request = route.request();
    if (request.method() === "OPTIONS") {
      await route.fulfill({
        status: 204,
        headers: {
          "access-control-allow-origin": "http://127.0.0.1:4173",
          "access-control-allow-headers": "authorization,content-type",
          "access-control-allow-methods": "GET,POST,OPTIONS",
        },
      });
      return;
    }
    expect(request.headers().authorization).toBe("Bearer e2e-iam-admin-token");
    const path = new URL(request.url()).pathname;
    if (path === "/api/iam/catalog") {
      await fulfillJson(route, catalog);
      return;
    }
    if (path === "/api/iam/change-requests" && request.method() === "GET") {
      await fulfillJson(route, [change]);
      return;
    }
    if (path.endsWith("/approve")) {
      change = {
        ...change,
        approvalCount: 1,
        status: "applied",
        approvals: [
          {
            approverId: "approver-id",
            approverDisplayName: "Mei Reviewer",
            decision: "approve",
            comment: "Verified scope and separation of duties",
            decidedAt: "2026-08-24T08:10:00Z",
          },
        ],
        version: 2,
      };
      await fulfillJson(route, change);
      return;
    }
    if (path === "/api/iam/change-requests" && request.method() === "POST") {
      const body = request.postDataJSON() as {
        operation: string;
        payload: Record<string, unknown>;
        reason: string;
      };
      change = {
        ...pendingChange(),
        id: "10000000-0000-4000-8000-000000000099",
        operation: body.operation,
        payload: body.payload,
        reason: body.reason,
      };
      await fulfillJson(route, change, 201);
      return;
    }
    await fulfillJson(route, { error: "not_found" }, 404);
  });
}

test.describe("Business IAM authority ledger", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      window.__BUZZ_E2E_WORKBENCH_ACCESS_TOKEN__ = "e2e-iam-admin-token";
    });
    await installIamApi(page);
    await installMockBridge(page);
    await page.goto("/");
    await expect(page.getByTestId("home-inbox-list")).toBeVisible();
  });

  test("applies a critical change after one review", async ({ page }) => {
    await page.getByTestId("business-iam-admin-toggle").click();
    const dialog = page.getByTestId("business-iam-admin-dialog");
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole("heading", {
        name: "Grant direct permission",
        exact: true,
      }),
    ).toBeVisible();
    await expect(dialog.getByTestId("iam-approval-rail")).toContainText(
      "Lin Requester",
    );
    await expect(dialog.getByText("No authority changed")).toBeVisible();

    await dialog
      .getByLabel("Review comment")
      .fill("Verified scope and separation of duties");
    await dialog.getByRole("button", { name: "Approve review" }).click();
    await expect(dialog.getByTestId("iam-approval-rail")).toContainText(
      "Mei Reviewer",
    );
    await expect(dialog.getByTestId("iam-approval-rail")).toHaveAccessibleName(
      "1 of 1 approvals complete",
    );
    await expect(dialog.getByText("Policy applied")).toBeVisible();
  });

  test("creates a version-bound request from catalog selections", async ({
    page,
  }) => {
    await page.getByTestId("business-iam-admin-toggle").click();
    const dialog = page.getByTestId("business-iam-admin-dialog");
    await dialog.getByRole("tab", { name: "New request" }).click();
    await expect(dialog.getByLabel("Step-up")).toHaveCount(0);
    await expect(dialog.getByLabel("Dual control")).toHaveCount(0);
    await expect(
      dialog.getByText(
        "Every request needs one review; the requester may approve it.",
      ),
    ).toBeVisible();
    await dialog
      .getByLabel("Principal", { exact: true })
      .selectOption("agent-id");
    await dialog
      .getByLabel("Capability", { exact: true })
      .selectOption("sales_order:write");
    await dialog
      .getByLabel("Scope", { exact: true })
      .selectOption("restricted");
    await dialog.getByLabel("Dimension", { exact: true }).fill("legal_entity");
    await dialog.getByLabel("Allowed values", { exact: true }).fill("cn");
    await dialog
      .getByLabel("Business reason")
      .fill("Grant China sales maintenance for the quarter-end process");
    await dialog.getByRole("button", { name: "Create review request" }).click();
    await expect(
      dialog.getByRole("tab", { name: "Review queue" }),
    ).toHaveAttribute("data-state", "active");
    await expect(
      dialog
        .getByTestId("iam-change-review")
        .getByText("Grant China sales maintenance for the quarter-end process"),
    ).toBeVisible();
  });
});
