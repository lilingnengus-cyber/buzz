import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/session", (route) => route.fulfill({json:{authenticated:true,subject:"fixture",displayName:"验收用户"}}));
});

for (const [side, title, workflow] of [
  ["sales", "销售退货", "pending"],
  ["purchase", "采购退货", "dispatched"],
]) {
  test(`${title}链接打开对应退货明细`, async ({ page }) => {
    const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
    const sourceId = "eef0b9f5-ec6a-4911-8ba0-5eaf1a3a3854";
    const requests: string[] = [];
    await page.route("**/api/v1/**", async (route) => {
      requests.push(route.request().method());
      expect(new URL(route.request().url()).pathname).toBe(
        `/api/v1/${side}-returns/${id}`,
      );
      await route.fulfill({
        json: {
          id,
          number: "RET-ACCEPTANCE",
          sourceId,
          status: "confirmed",
          workflowStatus: workflow,
          businessDate: "2026-09-19",
          currency: "CNY",
          version: 2,
          reasonCode: "QUALITY_ISSUE",
          businessNote: "<script>untrusted()</script>",
          amount: "200",
          cost: "100",
          lines: [
            {
              returnLineId: "line-1",
              skuCode: "SKU-1",
              skuName: "验收商品",
              quantity: "2.000000",
              unitCost: "50",
              totalCost: "100",
            },
          ],
        },
      });
    });
    await page.goto(`/embed/${side}-returns/${id}`);
    await expect(
      page.getByRole("heading", { name: `${title} · RET-ACCEPTANCE` }),
    ).toBeVisible();
    await expect(page.getByRole("table")).toContainText("验收商品");
    await expect(page.getByRole("table")).toContainText("100.00");
    await expect(page.getByText("退货原因：质量问题")).toBeVisible();
    await expect(
      page.getByText("备注：<script>untrusted()</script>"),
    ).toBeVisible();
    await expect(
      page.getByRole("link", {
        name: side === "sales" ? "查看关联出库单" : "查看关联收货单",
      }),
    ).toHaveAttribute(
      "href",
      `/embed/${side === "sales" ? "shipments" : "goods-receipts"}/${sourceId}`,
    );
    expect(requests.every((method) => method === "GET")).toBe(true);
  });
  test(`${title}未授权时不展示单据`, async ({ page }) => {
    await page.route("**/api/v1/**", (route) =>
      route.fulfill({ status: 404, json: { code: "not_found_or_forbidden" } }),
    );
    await page.goto(
      `/embed/${side}-returns/54a738b6-49ad-4c5b-9a08-6a16a0a119e2`,
    );
    await expect(
      page.getByRole("heading", { name: `当前账号无法访问${title}` }),
    ).toBeVisible();
    await expect(page.getByRole("table")).toHaveCount(0);
  });
}
