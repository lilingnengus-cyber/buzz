import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("订单详情汇总混合履约、回款、刷新失败与权限", async ({ page }) => {
  const order = { id: "mixed", orderNumber: "SO-MIXED", legalEntityId: "le", customerId: "customer", businessUnitId: "unit", currency: "CNY", lifecycleStatus: "confirmed", holdStatus: "none", fulfillmentStatus: "partially_fulfilled", grossAmount: "150", orderDate: "2026-10-03", updatedAt: "2026-10-03T00:00:00Z", version: 3 };
  const line = { lineNumber: 1, skuCode: "DEVICE", name: "设备", unit: "台", ordered: "5", delivered: "2", cancelled: "1", remaining: "2", complete: false, projectTitle: null, projectStatus: null };
  let mode = "normal";
  const writes: string[] = [];
  await page.route("**/api/**", async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    if (req.method() !== "GET") writes.push(req.method());
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf" } });
    if (path === "/api/v1/sales-orders") return route.fulfill({ json: { items: [order] } });
    if (path === "/api/v1/sales-orders/mixed") {
      if (mode === "error") return route.fulfill({ status: 503, json: { error: "retry" } });
      return route.fulfill({ json: { ...order, progress: { goods: mode === "restricted" ? null : [line], services: mode === "restricted" ? null : [{ ...line, lineNumber: 2, name: "实施服务", unit: "项", ordered: "1", delivered: "1", remaining: "0", cancelled: "0", complete: true, projectTitle: "企业上线实施", projectStatus: "completed" }], payment: mode === "restricted" ? null : { receivableCount: 2, amount: "100", settled: "40", open: "60", overdue: "20" }, dataAsOf: "2026-10-03T01:00:00Z" } } });
    }
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto('/#sales');
  await page.getByRole('button', { name: '查看 SO-MIXED 详情', exact: true }).click();
  const panel = page.getByRole('region', { name: '履约与回款进度', exact: true });
  await expect(panel.getByRole('region', { name: '商品交付', exact: true })).toContainText('已完成 0 / 1 行');
  await expect(panel.getByRole('region', { name: '服务验收', exact: true })).toContainText('企业上线实施');
  await expect(panel.getByRole('region', { name: '服务验收', exact: true })).toContainText('已完成 1 / 1 行');
  await expect(panel.getByRole('region', { name: '回款进度', exact: true })).toContainText('CNY 40.00');
  await waitForAnimations(page);
  await panel.screenshot({ path: 'test-results/sales-order-progress.png' });
  mode = 'error';
  await panel.getByRole('button', { name: '刷新进度' }).click();
  await expect(panel).toContainText('订单进度暂时不可用');
  await expect(panel).not.toContainText('CNY 40.00');
  mode = 'restricted';
  await panel.getByRole('button', { name: '重新加载' }).click();
  await expect(panel).toContainText('当前权限无法查看回款进度');
  await expect(panel).not.toContainText('CNY 0.00');
  await page.getByRole('button', { name: '关闭弹窗', exact: true }).click();
  mode = 'normal';
  await page.goto('/sales/orders/mixed');
  await expect(page.getByRole('heading', { name: '销售订单 · SO-MIXED', exact: true })).toBeVisible();
  await expect(panel).toContainText('CNY 40.00');
  expect(writes).toEqual([]);
});
