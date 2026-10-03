import { expect, test } from '@playwright/test';

test('库存健康查询部分失败时保留成功数据，失败不显示零并可重试', async ({ page }) => {
  let fails = true;
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/session') return route.fulfill({ json: { authenticated: true, csrfToken: 'csrf' } });
    if (path === '/api/v1/inventory-counts' && fails) return route.fulfill({ status: 403, json: {} });
    if (path === '/api/v1/inventory-aging') return route.fulfill({ json: { items: [{ warehouseId: 'w', skuId: 'sku', skuCode: 'SKU', skuName: '设备', currency: 'USD', inventoryValue: '50', onHandQuantity: '1', daysWithoutIssue: 120 }] } });
    if (path === '/api/v1/inventory-turnover') return route.fulfill({ json: { currency: 'CNY', turnoverRate: '0.50', turnoverDays: '60' } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto('/#inventory');
  await page.getByRole('button', { name: /期初与盘点/ }).click();
  const panel = page.locator('.inventory-control');
  await expect(panel).toContainText('盘点任务读取失败');
  await expect(panel).toContainText('USD 50.00');
  await expect(panel).toContainText('本月库存周转（CNY）');
  await expect(panel).not.toContainText('暂无盘点任务');
  fails = false;
  await panel.getByRole('button', { name: '重新读取库存健康' }).click();
  await expect(panel).toContainText('暂无盘点任务');
  await expect(panel).not.toContainText('读取失败');
});

test('库龄币种不明时不标为 CNY 且不计入已知币种金额', async ({ page }) => {
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/session') return route.fulfill({ json: { authenticated: true, csrfToken: 'csrf' } });
    if (path === '/api/v1/inventory-aging') return route.fulfill({ json: { items: [
      { warehouseId: 'w', skuId: 'a', skuCode: 'KNOWN', skuName: '已知', currency: 'USD', inventoryValue: '50', onHandQuantity: '1', daysWithoutIssue: 120 },
      { warehouseId: 'w', skuId: 'b', skuCode: 'MIXED', skuName: '混币', currency: null, inventoryValue: '999', onHandQuantity: '2', daysWithoutIssue: 120 },
    ] } });
    if (path === '/api/v1/inventory-turnover') return route.fulfill({ json: { currency: 'CNY', turnoverRate: null, turnoverDays: null, excludedCurrencyBalances: 1 } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto('/#inventory');
  await page.getByRole('button', { name: /期初与盘点/ }).click();
  const panel = page.locator('.inventory-control');
  await expect(panel).toContainText('币种待核对记录未计入金额');
  await expect(panel).toContainText('暂不计算周转率');
  await expect(panel).toContainText('USD 50.00');
  await panel.locator('.aging-register summary').click();
  await expect(panel).toContainText('币种待核对');
  await expect(panel).not.toContainText('999.00');
});
