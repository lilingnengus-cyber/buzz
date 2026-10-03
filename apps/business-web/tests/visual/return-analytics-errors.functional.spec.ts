import { expect, test } from "@playwright/test";

test("退货指标失败不显示零金额，重试后展示实际结果", async ({ page }) => {
  let attempts = 0;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "test", displayName: "验收" } });
    if (path === "/api/v1/return-analytics") {
      attempts += 1;
      if (attempts === 1) return route.fulfill({ status: 503, json: { error: "暂不可用" } });
      return route.fulfill({ json: { items: [{ salesReturnAmount: "20", purchaseReturnAmount: "0", shippedSalesAmount: "200", receivedPurchaseAmount: "0", returnLossAmount: "5", scrapCostAmount: "1" }] } });
    }
    return route.fulfill({ json: { items: [], hasMore: false, canManage: true } });
  });
  await page.goto("/#goodsOrders");
  await page.getByRole("navigation", { name: "订单闭环阶段" }).getByRole("button", { name: /销售退货/ }).click();
  const panel = page.getByRole("region", { name: "本月退货经营指标" });
  await expect(panel.getByRole("alert")).toContainText("读取失败");
  await expect(panel).not.toContainText("0.00");
  await panel.getByRole("button", { name: "重试退货指标" }).click();
  await expect(panel).toContainText("CNY 20.00");
  await expect(panel).toContainText("10.00%");
  await expect(panel.getByRole("alert")).toHaveCount(0);
});
