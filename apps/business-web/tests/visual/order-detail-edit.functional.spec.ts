import { expect, test } from "@playwright/test";

for (const side of ["sales", "purchase"] as const) {
  test(`${side} 记录详情进入编辑、外部关闭、已确认只读`, async ({ page }) => {
    const number = side === "sales" ? "SO-DETAIL" : "PO-DETAIL";
    const draft = {
      id: "draft",
      orderNumber: number,
      purchaseOrderNumber: number,
      legalEntityId: "legal",
      businessUnitId: "unit",
      customerId: "customer",
      supplierId: "supplier",
      lifecycleStatus: "draft",
      holdStatus: "none",
      fulfillmentStatus: "unreserved",
      receivingStatus: "unreceived",
      currency: "CNY",
      grossAmount: "12",
      orderDate: "2026-10-02",
      updatedAt: "2026-10-02T00:00:00Z",
      version: 3,
      paymentTermsDays: 30,
      customerReference: "SO-REF",
      supplierReference: "PO-REF",
      lines: [
        {
          skuId: "sku",
          warehouseId: "warehouse",
          unitOfMeasureId: "uom",
          quantity: "1",
          unitPrice: "12",
          discountAmount: "0",
          taxRate: "0",
        },
      ],
    };
    let writes = 0;
    await page.route("**/api/**", async (route) => {
      const req = route.request(),
        path = new URL(req.url()).pathname;
      if (["PUT", "POST"].includes(req.method())) writes++;
      if (path === "/api/session")
        await route.fulfill({
          json: { authenticated: true, subject: "test", csrfToken: "csrf" },
        });
      else if (
        path.endsWith("/draft-options") ||
        path.endsWith("/entry-options")
      )
        await route.fulfill({
          json: { canCreate: true, canUpdate: true, draft },
        });
      else if (path === `/api/v1/${side}-orders`)
        await route.fulfill({
          json: {
            items: [
              draft,
              {
                ...draft,
                id: "confirmed",
                orderNumber: "CONFIRMED",
                purchaseOrderNumber: "CONFIRMED",
                lifecycleStatus: "confirmed",
              },
            ],
          },
        });
      else await route.fulfill({ json: { items: [] } });
    });
    await page.goto(side === "sales" ? "/#sales" : "/#purchasing");
    await page
      .getByRole("button", { name: `查看 ${number} 详情`, exact: true })
      .click();
    let dialog = page.getByRole("dialog");
    await expect(dialog).toContainText(number);
    await dialog.getByRole("heading").click();
    await expect(dialog).toBeVisible();
    await page.mouse.click(10, 10);
    await expect(dialog).toHaveCount(0);
    await page
      .getByRole("button", { name: `查看 ${number} 详情`, exact: true })
      .click();
    await dialog.getByRole("button", { name: "编辑草稿", exact: true }).click();
    await expect(dialog).toHaveAccessibleName(`${number} · 编辑草稿`);
    await expect(
      dialog.getByRole("textbox", {
        name: side === "sales" ? "客户参考号" : "供应商参考号",
      }),
    ).toHaveValue(side === "sales" ? "SO-REF" : "PO-REF");
    await page.mouse.click(10, 10);
    await expect(dialog).toHaveCount(0);
    expect(writes).toBe(0);
    await page
      .getByRole("button", { name: "查看 CONFIRMED 详情", exact: true })
      .click();
    await expect(
      dialog.getByRole("button", { name: "编辑草稿", exact: true }),
    ).toHaveCount(0);
    await expect(dialog).toContainText("只读详情");
  });
}
