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

for (const side of ["sales","purchase"]) {
  test(`${side} 冲销详情保留原单并展示历史变化`, async ({ page }) => {
    const id="54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
    await page.route("**/api/v1/**",route=>route.fulfill({json:{id,number:"RET-REVERSED",sourceId:id,status:"reversed",workflowStatus:"pending",businessDate:"2026-09-19",currency:"CNY",version:3,reasonCode:"QUALITY_ISSUE",amount:"100",cost:"50",lines:[{returnLineId:"l",skuId:"sku",skuCode:"SKU-R",skuName:"退货商品",quantity:"1",unitCost:"50",totalCost:"50"}],reversal:{date:"2026-10-21",reason:"<script>错误登记</script>",version:3,financial:{originalAmountBefore:"100",originalAmountAfter:"200",openAmountBefore:"100",openAmountAfter:"200",settledAmount:"0"},inventory:[{skuId:"sku",onHandQuantityBefore:"1",onHandQuantityAfter:"0",quarantinedQuantityBefore:"1",quarantinedQuantityAfter:"0",inventoryValueBefore:"50",inventoryValueAfter:"0"}]}}}));
    await page.goto(`/embed/${side}-returns/${id}`);
    const record=page.getByRole("region",{name:"冲销记录"});
    await expect(record).toContainText("冲销日期：2026-10-21");
    await expect(record).toContainText("冲销原因：<script>错误登记</script>");
    await expect(record).toContainText(`${side==="sales"?"应收":"应付"}未结余额：100.00 → 200.00`);
    await expect(record.getByRole("table")).toContainText("SKU-R · 退货商品");
    await expect(record.getByRole("table")).toContainText("50.00 → 0.00");
    await expect(page.getByText("原退货与冲销记录均已保留",{exact:false})).toBeVisible();
    await expect(record.getByRole("button")).toHaveCount(0);
  });
}
