import { expect, test } from "@playwright/test";

test("期初库存链接读取对应单据且不提供过账按钮", async ({ page }) => {
  const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
  const writes: string[] = [];
  await page.route("**/api/v1/**", async (route) => {
    if (route.request().method() !== "GET") writes.push(route.request().url());
    expect(new URL(route.request().url()).pathname).toBe(
      `/api/v1/inventory-openings/${id}`,
    );
    await route.fulfill({
      json: {
        number: "OPEN-ACCEPTANCE",
        status: "draft",
        businessDate: "2026-09-19",
        currency: "CNY",
        version: 1,
        lines: [
          {
            warehouseName: "验收仓库",
            skuName: "验收商品",
            quantity: "2.000000",
            unitCost: "50.000000",
            totalCost: "100.000000",
          },
        ],
      },
    });
  });
  await page.goto(`/embed/inventory-openings/${id}`);
  await expect(
    page.getByRole("heading", { name: "期初库存 · OPEN-ACCEPTANCE" }),
  ).toBeVisible();
  await expect(page.getByRole("table")).toContainText("验收仓库");
  await expect(page.getByRole("table")).toContainText("100.00");
  await expect(
    page.getByRole("button", { name: "过账", exact: true }),
  ).toHaveCount(0);
  expect(writes).toEqual([]);
});
