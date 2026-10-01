import { expect, test } from "@playwright/test";

for (const kind of ["sales", "purchase"] as const) {
  test(`${kind} 删除草稿先确认，取消无写入，失败可重试`, async ({ page }) => {
    const purchase = kind === "purchase";
    const label = purchase ? "采购" : "销售";
    const requests: { body: unknown; key: string | undefined }[] = [];
    let deleted = false;
    const draft = {
      id: "draft",
      orderNumber: "SO-DRAFT",
      purchaseOrderNumber: "PO-DRAFT",
      legalEntityId: "legal",
      businessUnitId: "unit",
      customerId: "customer",
      supplierId: "supplier",
      lifecycleStatus: "draft",
      holdStatus: "none",
      fulfillmentStatus: "unreserved",
      receivingStatus: "unreceived",
      grossAmount: "1",
      currency: "CNY",
      orderDate: "2026-10-01",
      updatedAt: "2026-10-01T00:00:00Z",
      version: 3,
    };
    await page.route("**/api/**", async (route) => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (path === "/api/session") {
        await route.fulfill({
          json: {
            authenticated: true,
            subject: "tester",
            displayName: "验收",
            csrfToken: "csrf",
          },
        });
      } else if (path.endsWith("/delete-draft")) {
        requests.push({
          body: request.postDataJSON(),
          key: request.headers()["idempotency-key"],
        });
        if (requests.length === 1) {
          await route.fulfill({
            status: 409,
            json: {
              error: {
                code: "VERSION_CONFLICT",
                message: "订单版本已变化，请刷新后重试。",
              },
            },
          });
        } else {
          deleted = true;
          await route.fulfill({
            json: { id: "draft", status: "deleted", version: 4 },
          });
        }
      } else if (path === `/api/v1/${kind}-orders`) {
        await route.fulfill({
          json: {
            items: [
              ...(!deleted ? [draft] : []),
              {
                ...draft,
                id: "confirmed",
                lifecycleStatus: "confirmed",
                orderNumber: "SO-CONFIRMED",
                purchaseOrderNumber: "PO-CONFIRMED",
              },
            ],
          },
        });
      } else {
        await route.fulfill({ json: { items: [] } });
      }
    });
    await page.goto(purchase ? "/#purchasing" : "/#sales");
    await expect(
      page.getByRole("button", {
        name: `删除${label}订单 ${purchase ? "PO" : "SO"}-CONFIRMED`,
        exact: true,
      }),
    ).toHaveCount(0);
    const remove = page.getByRole("button", {
      name: `删除${label}订单 ${purchase ? "PO" : "SO"}-DRAFT`,
      exact: true,
    });
    await remove.click();
    const dialog = page.getByRole("dialog", { name: /删除.*订单草稿/ });
    await expect(dialog).toContainText("订单编号和审计记录保留");
    expect(requests).toHaveLength(0);
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
    expect(requests).toHaveLength(0);
    await expect(remove).toBeVisible();
    await remove.click();
    await dialog.getByRole("button", { name: "确认删除", exact: true }).click();
    await expect(dialog.getByRole("alert")).toBeVisible();
    expect(requests[0].body).toEqual({ expectedVersion: 3 });
    expect(requests[0].key).toBeTruthy();
    await expect(remove).toHaveCount(1);
    await dialog.getByRole("button", { name: "确认删除", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(remove).toHaveCount(0);
    expect(requests).toHaveLength(2);
  });
}
