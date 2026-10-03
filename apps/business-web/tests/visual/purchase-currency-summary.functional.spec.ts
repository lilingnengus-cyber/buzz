import { expect, test } from "@playwright/test";

test("采购金额按币种汇总、跟随订单筛选并排除撤销应付", async ({ page }) => {
  const common = { supplierId: 'supplier', legalEntityId: 'legal', businessUnitId: 'unit', lifecycleStatus: 'draft', receivingStatus: 'unreceived', orderDate: '2026-10-03', updatedAt: '2026-10-03T00:00:00Z', version: 1 };
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/session') return route.fulfill({ json: { authenticated: true, csrfToken: 'csrf' } });
    if (path === '/api/v1/purchase-orders') return route.fulfill({ json: { items: [
      { ...common, id: 'cny', purchaseOrderNumber: 'PO-CNY', currency: 'CNY', grossAmount: '100' },
      { ...common, id: 'usd', purchaseOrderNumber: 'PO-USD', currency: 'USD', grossAmount: '20' },
    ] } });
    if (path === '/api/v1/trade-payables') return route.fulfill({ json: { items: [
      { ...common, id: 'a', payableNumber: 'AP-CNY', currency: 'CNY', originalAmount: '100', openAmount: '80', status: 'open' },
      { ...common, id: 'b', payableNumber: 'AP-USD', currency: 'USD', originalAmount: '20', openAmount: '10', status: 'open' },
      { ...common, id: 'c', payableNumber: 'AP-VOID', currency: 'CNY', originalAmount: '999', openAmount: '999', status: 'reversed' },
    ] } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto('/#purchasing');
  const totals = page.locator('.workflow-pulse');
  await expect(totals).toContainText('CNY 100.00 / USD 20.00');
  await expect(totals).toContainText('CNY 80.00 / USD 10.00');
  await expect(totals).not.toContainText('999');
  await page.getByRole('searchbox', { name: '搜索业务单据' }).fill('PO-USD');
  await expect(totals).toContainText('USD 20.00');
  await expect(totals).not.toContainText('CNY 100.00');
  await expect(totals).toContainText('筛选后 1 张订单');
});
