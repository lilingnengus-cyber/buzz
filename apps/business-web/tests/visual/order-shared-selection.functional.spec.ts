import { expect, test } from "@playwright/test";

for (const flow of ["sales", "purchase"] as const) {
  test(`${flow} 法人切换保留每行共享仓库及往来方`, async ({ page }) => {
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/session") {
        await route.fulfill({
          json: { authenticated: true, subject: "tester", csrfToken: "test" },
        });
      } else if (path.endsWith("/entry-options")) {
        await route.fulfill({
          json: { canCreate: true, canUpdate: true, draft: null },
        });
      } else if (path.startsWith("/api/v1/master-data/")) {
        const resource = path.split("/").at(-1) ?? "";
        const items = [1, 2].map((index) => ({
          id: `${resource}-${index}`,
          code: `${resource}-${index}`,
          name: `${resource}-${index}`,
          status: "active",
          resourceType: resource,
          legalEntityId: null,
        }));
        await route.fulfill({ json: { items } });
      } else if (path === "/api/v1/core-master-data") {
        await route.fulfill({
          json: {
            items: [
              {
                id: "bu",
                code: "BU",
                name: "经营主体",
                resourceType: "business_unit",
                status: "active",
                parentBusinessUnitId: null,
                ancestorPath: ["经营主体"],
                depth: 0,
                descendantCount: 0,
              },
            ],
          },
        });
      } else {
        await route.fulfill({ json: { items: [] } });
      }
    });
    const purchase = flow === "purchase";
    await page.goto(purchase ? "/#purchasing" : "/#sales");
    const title = purchase ? "新增采购订单" : "新增销售订单";
    await page.getByRole("button", { name: title, exact: true }).click();
    const dialog = page.getByRole("dialog", { name: title });
    const party = dialog.getByLabel(purchase ? "供应商" : "客户", {
      exact: true,
    });
    await party.selectOption(`${purchase ? "supplier" : "customer"}-2`);
    const warehouseLabel = purchase ? "收货仓库" : "仓库";
    await dialog
      .getByLabel(`第 1 行${warehouseLabel}`, { exact: true })
      .selectOption("warehouse-2");
    await dialog
      .getByRole("button", { name: purchase ? "+ 添加采购行" : "+ 添加商品行" })
      .click();
    await dialog
      .getByLabel("法律主体", { exact: true })
      .selectOption("legal_entity-2");
    await expect(party).toHaveValue(`${purchase ? "supplier" : "customer"}-2`);
    await expect(
      dialog.getByLabel(`第 1 行${warehouseLabel}`, { exact: true }),
    ).toHaveValue("warehouse-2");
    await expect(
      dialog.getByLabel(`第 2 行${warehouseLabel}`, { exact: true }),
    ).toHaveValue("warehouse-1");
  });
}
