import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

for (const width of [1366, 520]) {
  test(`商机关联订单按需读取汇总，币种和权限独立，失败清除旧值 ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const reads: string[] = [];
    const writes: string[] = [];
    let fails = false;
    let restricted = true;
    const orders = [
      { id: "goods", orderNumber: "SO-CNY", currency: "CNY", grossAmount: "110", lifecycleStatus: "confirmed", orderDate: "2026-10-03" },
      { id: "service", orderNumber: "SO-USD", currency: "USD", grossAmount: "50", lifecycleStatus: "completed", orderDate: "2026-10-03" },
    ];
    const source = { id: "opp", title: "混合采购", companyName: "测试客户", customerId: "customer", stage: "won", currency: "CNY", version: 1 };
    const line = { lineNumber: 1, skuCode: "DEVICE", name: "设备", unit: "台", ordered: "1", delivered: "0", cancelled: "0", remaining: "1", complete: false, projectTitle: null, projectStatus: null };
    await page.route("**/api/**", async (route) => {
      const req = route.request(), path = new URL(req.url()).pathname;
      if (req.method() !== "GET") writes.push(path);
      if (path === "/api/session") return route.fulfill({ json: { authenticated: true } });
      if (path === "/api/v1/crm/opportunities") return route.fulfill({ json: { items: [source], canManage: false, hasMore: false } });
      if (path === "/api/v1/crm/opportunities/opp") return route.fulfill({ json: { item: source, followups: [], hasOlderFollowups: false } });
      if (path === "/api/v1/sales-orders") {
        expect(new URL(req.url()).searchParams.get("opportunityId")).toBe("opp");
        return route.fulfill({ json: { items: orders } });
      }
      if (path.startsWith("/api/v1/sales-orders/")) {
        reads.push(path);
        if (path.endsWith("goods") && fails) return route.fulfill({ status: 503, json: { message: "暂不可用" } });
        const denied = path.endsWith("service") && restricted;
        return route.fulfill({ json: { currency: path.endsWith("goods") ? "CNY" : "USD", progress: {
          goods: denied ? null : [line],
          services: denied ? null : [{ ...line, complete: true, delivered: "1", remaining: "0" }],
          payment: denied ? null : { receivableCount: 2, amount: "110", settled: "40", open: "70", overdue: "20" },
          dataAsOf: "2026-10-03T01:00:00Z",
        } } });
      }
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto("/#crm?opportunity=opp");
    const drawer = page.getByRole("dialog");
    const goods = drawer.getByRole("article", { name: "关联订单 SO-CNY" });
    const service = drawer.getByRole("article", { name: "关联订单 SO-USD" });
    await expect(goods.getByRole("link")).toHaveAttribute("href", "/sales/orders/goods");
    await expect(service).toContainText("美元 50.00");
    expect(reads).toEqual([]);
    await goods.locator("summary").click();
    await expect(goods.getByRole("region", { name: "商品交付" })).toContainText("已完成 0 / 1 行");
    await expect(goods.getByRole("region", { name: "服务验收" })).toContainText("已完成 1 / 1 行");
    await expect(goods.getByRole("region", { name: "回款进度", exact: true })).toContainText("人民币 40.00");
    await expect(goods.getByRole("table")).toHaveCount(0);
    expect(reads).toEqual(["/api/v1/sales-orders/goods"]);
    await service.locator("summary").click();
    await expect(service).toContainText("当前权限无法查看回款进度");
    await expect(service).toContainText("当前权限或数据范围不足");
    await expect(service).not.toContainText("美元 0.00");
    fails = true;
    await goods.getByRole("button", { name: "刷新进度" }).click();
    await expect(goods.getByRole("alert")).toContainText("订单进度暂时不可用");
    await expect(goods).not.toContainText("人民币 40.00");
    await expect(service).toContainText("当前权限无法查看回款进度");
    fails = false;
    await goods.getByRole("button", { name: "重新加载", exact: true }).click();
    await expect(goods.getByRole("region", { name: "回款进度", exact: true })).toContainText("人民币 40.00");
    restricted = false;
    await service.getByRole("button", { name: "刷新进度" }).click();
    await expect(service.getByRole("region", { name: "回款进度", exact: true })).toContainText("美元 40.00");
    await waitForAnimations(page);
    await drawer.screenshot({ path: `test-results/crm-order-summary-${width}.png` });
    expect(await drawer.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await drawer.getByRole("button", { name: "刷新关联订单" }).click();
    await expect(drawer.getByRole("region", { name: "履约与回款进度", exact: true })).toHaveCount(0);
    await expect(drawer.getByRole("article")).toHaveCount(2);
    expect(writes).toEqual([]);
  });
}
