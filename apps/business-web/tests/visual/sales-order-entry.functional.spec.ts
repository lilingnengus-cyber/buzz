import { expect, type Page, test } from "@playwright/test";

const masters: Record<string, { id: string; code: string; name: string }> = {
  legal_entity: { id: "legal-entity-1", code: "LE-01", name: "法律主体" },
  customer: { id: "customer-1", code: "CUS-01", name: "客户" },
  business_unit: { id: "business-unit-1", code: "BU-01", name: "业务单元" },
  sku: { id: "sku-1", code: "SKU-001", name: "测试商品" },
  warehouse: { id: "warehouse-1", code: "WH-01", name: "测试仓库" },
  unit_of_measure: { id: "uom-1", code: "PCS", name: "件" },
};

async function installFixtures(page: Page) {
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "functional-user",
          displayName: "功能测试",
          csrfToken: "functional-csrf",
        },
      });
      return;
    }
    if (url.pathname.startsWith("/api/v1/master-data/")) {
      const resource = url.pathname.split("/").at(-1) ?? "";
      const value = masters[resource];
      await route.fulfill({
        json: {
          items: value
            ? [
                {
                  resourceType: resource,
                  ...value,
                  status: "active",
                  legalEntityId:
                    resource === "legal_entity" ? null : "legal-entity-1",
                },
              ]
            : [],
          scopeVersion: 1,
          effectiveScopeHash: "functional-fixture",
        },
      });
      return;
    }
    await route.fulfill({ json: { items: [] } });
  });
}

test("销售草稿要求显式填写单价并原样提交", async ({ page }) => {
  const submitted: unknown[] = [];
  await installFixtures(page);
  page.on("request", (request) => {
    if (
      request.method() === "POST" &&
      new URL(request.url()).pathname === "/api/v1/sales-orders"
    ) {
      submitted.push(request.postDataJSON());
    }
  });

  await page.goto("/#sales");
  await page.getByRole("button", { name: "新增销售订单" }).click();
  const dialog = page.getByRole("dialog", { name: "新增销售订单" });
  const unitPrice = dialog.getByRole("spinbutton", { name: "第 1 行单价" });
  const save = dialog.getByRole("button", { name: "保存销售订单草稿" });

  await expect(unitPrice).toHaveValue("");
  await expect(unitPrice).toHaveAttribute("placeholder", "必填");
  await save.click();
  expect(submitted).toHaveLength(0);

  await unitPrice.fill("1.00");
  await save.click();
  await expect.poll(() => submitted.length).toBe(1);
  expect(submitted[0]).toMatchObject({
    lines: [{ quantity: "1", unitPrice: "1.00" }],
  });
});
