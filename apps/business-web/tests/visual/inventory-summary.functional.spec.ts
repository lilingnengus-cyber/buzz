import { expect, test } from '@playwright/test';

test('库存汇总分单位与币种、跟随筛选，混用币种不伪造总额', async ({ page }) => {
  const base = { legalEntityId: 'le', warehouseId: 'wh', reservedQuantity: '0', quarantinedQuantity: '0', averageUnitCost: '10', updatedAt: '2026-10-03T00:00:00Z', version: 1 };
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/session') return route.fulfill({ json: { authenticated: true } });
    if (path === '/api/v1/inventory-balances') return route.fulfill({ json: { items: [
      { ...base, skuId: 'pieces', unitOfMeasureId: 'piece', unitName: '件', onHandQuantity: '2', availableQuantity: '2', currency: 'CNY', inventoryValue: '20' },
      { ...base, skuId: 'weight', unitOfMeasureId: 'kg', unitName: '公斤', onHandQuantity: '3', availableQuantity: '3', currency: 'USD', inventoryValue: '30' },
      { ...base, skuId: 'mixed', unitOfMeasureId: 'piece', unitName: '件', onHandQuantity: '1', availableQuantity: '1', currency: null, currencyConflict: true, inventoryValue: '999' },
    ] } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto('/#inventory');
  const summary = page.getByRole('region', { name: '当前库存汇总' });
  await expect(summary).toContainText('3.00 件');
  await expect(summary).toContainText('3.00 公斤');
  await expect(summary).toContainText('CNY 20.00');
  await expect(summary).toContainText('USD 30.00');
  await expect(summary).toContainText('1 条余额币种缺失或混用');
  await expect(summary).not.toContainText('999');
  await expect(page.getByRole('row').filter({ hasText: 'mixed' })).toContainText('币种待核对');
  await page.getByRole('textbox', { name: '搜索商品或仓库' }).fill('weight');
  await expect(summary).toContainText('3.00 公斤');
  await expect(summary).not.toContainText('CNY');
  await expect(summary).not.toContainText('混用');
});
