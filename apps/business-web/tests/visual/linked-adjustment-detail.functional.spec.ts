import { expect, test } from "@playwright/test";
const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
const order = "64a738b6-49ad-4c5b-9a08-6a16a0a119e2";
function detail(offset: number, version = 3) {
  return { schemaVersion: 1, batch: { id, adjustment_number: "ADJ-DETAIL", status: "posted", management_period: "2026-09", currency: "CNY", version },
    version, boundary: "management_only_not_general_ledger", totalAmount: "30.00", targetOrderCount: 1,
    lines: [{ id: `line-${offset}`, metric_type: "outbound_freight", amount: offset ? "20.00" : "10.00", currency: "CNY", business_date: "2026-09-20", allocation_basis: "direct", reason_code: offset ? "第二条原因" : "第一条原因", direct_sales_order_id: order }],
    pagination: { offset, total: 2, nextOffset: offset ? null : 1 } };
}
for (const embed of [true, false]) {
  test(`费用详情完整分页 ${embed ? "嵌入" : "浏览器"}`, async ({ page }) => {
    const seen: string[] = [];
    await page.route("**/api/v1/**", async route => {
      expect(route.request().method()).toBe("GET");
      const url = new URL(route.request().url());
      expect(url.pathname).toBe(`/api/v1/profit-adjustments/${id}`);
      seen.push(url.search);
      const offset = Number(url.searchParams.get("offset"));
      if (offset) expect(url.searchParams.get("expectedVersion")).toBe("3");
      await route.fulfill({ json: detail(offset) });
    });
    const prefix = embed ? "/embed" : "";
    await page.goto(`${prefix}/profit-adjustments/${id}`);
    await expect(page.getByRole("heading", { name: "经营费用 · ADJ-DETAIL" })).toBeVisible();
    await expect(page.getByRole("table")).toContainText("第二条原因");
    await expect(page.getByTestId("adjustment-detail")).toContainText("CNY 30.00");
    await expect(page.getByRole("link", {name:"查看订单"}).first()).toHaveAttribute("href",`${prefix}/sales-orders/${order}`);
    await expect(page.getByRole("button", {name:"冲销", exact:true})).toHaveCount(0);
    expect(seen).toHaveLength(2);
  });
}
test("第二页版本变化不展示部分明细", async ({page}) => {
  await page.route("**/api/v1/**", async route => {
    const offset=Number(new URL(route.request().url()).searchParams.get("offset"));
    await route.fulfill({json: detail(offset, offset ? 4 : 3)});
  });
  await page.goto(`/embed/profit-adjustments/${id}`);
  await expect(page.getByRole("alert")).toContainText("费用明细已变化");
  await expect(page.getByRole("table")).toHaveCount(0);
});
test("无权限不回退列表", async ({page}) => {
  await page.route("**/api/v1/**", async route => {
    expect(new URL(route.request().url()).pathname).toBe(`/api/v1/profit-adjustments/${id}`);
    await route.fulfill({status:403,json:{code:"FORBIDDEN"}});
  });
  await page.goto(`/embed/profit-adjustments/${id}`);
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page.getByRole("table")).toHaveCount(0);
});
