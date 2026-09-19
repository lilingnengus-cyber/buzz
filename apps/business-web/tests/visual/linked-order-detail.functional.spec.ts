import { expect, test } from "@playwright/test";

for (const domain of ["sales", "purchase"] as const) {
  test(`${domain}链接直接读取指定订单，权限失败后可以重试`, async ({
    page,
  }) => {
    const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
    let allowed = false;
    const requests: string[] = [];
    await page.route("**/api/v1/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      requests.push(path);
      if (path !== `/api/v1/${domain}-orders/${id}`) {
        await route.fulfill({ status: 500, json: {} });
        return;
      }
      if (!allowed) {
        await route.fulfill({
          status: 404,
          json: {
            code: "not_found_or_forbidden",
            message: "resource was not found or is not accessible",
          },
        });
        return;
      }
      await route.fulfill({
        json: {
          id,
          orderNumber: "SO-LINK-0001",
          purchaseOrderNumber: "PO-LINK-0001",
          customerId: id,
          supplierId: id,
          legalEntityId: id,
          currency: "CNY",
          grossAmount: "23.45",
          lifecycleStatus: "draft",
          holdStatus: "none",
          fulfillmentStatus: "not_started",
          receivingStatus: "not_started",
          orderDate: "2026-09-19",
          updatedAt: "2026-09-19T00:00:00Z",
          version: 1,
        },
      });
    });
    await page.goto(`/embed/${domain}-orders/${id}`);
    await expect(page.getByRole("alert")).toHaveAttribute(
      "data-failure-kind",
      "access_denied",
    );
    allowed = true;
    await page.getByRole("button", { name: "重新检查" }).click();
    await expect(page.getByTestId("workflow-record-detail")).toContainText(
      domain === "sales" ? "SO-LINK-0001" : "PO-LINK-0001",
    );
    await expect(page.getByTestId("workflow-record-detail")).toContainText(
      "23.45",
    );
    expect(requests).toEqual([
      `/api/v1/${domain}-orders/${id}`,
      `/api/v1/${domain}-orders/${id}`,
    ]);
  });
}
