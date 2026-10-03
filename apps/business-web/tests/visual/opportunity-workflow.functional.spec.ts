import { expect, test } from "@playwright/test";

for (const mode of ["goods", "service"] as const) {
  test(`商机筛选闭环 ${mode} 保留范围、刷新与清除，失败不读取全部`, async ({ page }) => {
    let denied = false;
    const queries: string[] = [];
    const source = { id: "opp", title: "专项采购", companyName: "客户", customerId: "c", stage: "won", currency: "CNY", version: 1 };
    const line = { lineNumber: 1, skuCode: "SKU", name: "测试明细", unit: "项", ordered: "1", delivered: "1", cancelled: "0", remaining: "0", complete: true, projectTitle: "实施项目", projectStatus: "completed" };
    await page.route("**/api/**", async (route) => {
      const req = route.request(), url = new URL(req.url()), path = url.pathname;
      expect(req.method()).toBe("GET");
      if (path === "/api/session") return route.fulfill({ json: { authenticated: true } });
      if (path === "/api/v1/crm/opportunities") return route.fulfill({ json: { items: [source], canManage: false, hasMore: false } });
      if (path === "/api/v1/crm/opportunities/opp") return denied
        ? route.fulfill({ status: 403, json: { message: "不可访问" } })
        : route.fulfill({ json: { item: source, followups: [], hasOlderFollowups: false } });
      if (path === "/api/v1/sales-orders") {
        queries.push(url.search);
        return route.fulfill({ json: { items: [{ id: "order", orderNumber: "SO-SCOPED", orderDate: "2026-10-03", currency: "CNY", grossAmount: "10", lifecycleStatus: "completed" }] } });
      }
      if (path === "/api/v1/sales-orders/order") return route.fulfill({ json: { currency: "CNY", progress: { goods: [line], services: [line], payment: { receivableCount: 1, amount: "10", settled: "10", open: "0", overdue: "0" }, dataAsOf: "2026-10-03T01:00:00Z" } } });
      return route.fulfill({ json: { items: [], canManage: false } });
    });
    await page.goto('/#crm?opportunity=opp');
    const label = mode === "goods" ? "商品" : "服务";
    await page.getByRole("link", { name: `查看此商机${label}订单闭环` }).click();
    const section = mode === "goods" ? "goodsOrders" : "serviceOrders";
    await expect(page).toHaveURL(new RegExp(`#${section}\\?opportunity=opp`));
    await expect(page.getByText("商机筛选：专项采购")).toBeVisible();
    const card = page.getByRole("article", { name: "关联订单 SO-SCOPED" });
    await card.locator("summary").click();
    await expect(card.getByRole("region", { name: mode === "goods" ? "商品交付" : "服务验收", exact: true })).toBeVisible();
    await expect(card.getByRole("region", { name: mode === "goods" ? "服务验收" : "商品交付", exact: true })).toHaveCount(0);
    await page.reload();
    await expect(page.getByText("商机筛选：专项采购")).toBeVisible();
    await expect(card).toBeVisible();
    expect(queries.every(q => new URLSearchParams(q).get("opportunityId") === "opp")).toBe(true);
    denied = true;
    const previous = queries.length;
    await page.reload();
    await expect(page.getByRole("alert")).toContainText("当前账号无法访问商机");
    await expect(card).toHaveCount(0);
    expect(queries.length).toBe(previous);
    denied = false;
    await page.getByRole("button", { name: "重新检查" }).click();
    await expect(card).toBeVisible();
    await page.getByRole("link", { name: "清除商机筛选" }).click();
    await expect(page).toHaveURL(new RegExp(`#${section}$`));
    await expect(page.getByRole("navigation", { name: "订单闭环阶段" })).toBeVisible();
    await expect(page.getByText("商机筛选：专项采购")).toHaveCount(0);
  });
}
