import type { InventoryBalance } from './api';
import { formatMoney, formatQuantity } from './formatters';

export function InventorySummary({ rows }: { rows: InventoryBalance[] }) {
  const units = new Map<string, { name: string; onHand: number; reserved: number; quarantined: number; available: number }>();
  const currencies = new Map<string, number>();
  let unknownCurrency = 0;
  for (const row of rows) {
    const key = row.unitOfMeasureId ?? `sku:${row.skuId}`;
    const unit = units.get(key) ?? { name: row.unitName ?? '单位待核对', onHand: 0, reserved: 0, quarantined: 0, available: 0 };
    unit.onHand += Number(row.onHandQuantity);
    unit.reserved += Number(row.reservedQuantity);
    unit.quarantined += Number(row.quarantinedQuantity);
    unit.available += Number(row.availableQuantity);
    units.set(key, unit);
    if (row.currency && !row.currencyConflict) currencies.set(row.currency, (currencies.get(row.currency) ?? 0) + Number(row.inventoryValue));
    else unknownCurrency++;
  }
  const quantity = (key: 'onHand' | 'reserved' | 'quarantined' | 'available') => units.size ? [...units.entries()].map(([id, unit]) => <span className="inventory-summary-amount" key={id}>{formatQuantity(unit[key])} {unit.name}</span>) : '—';
  return <section className="inventory-equation" aria-label="当前库存汇总">
    <div className="inventory-equation-item on-hand"><span>在手库存</span><strong>{quantity('onHand')}</strong><small>已加载筛选结果，分计量单位汇总</small></div>
    <i>−</i>
    <div className="inventory-equation-item reserved"><span>销售预占</span><strong>{quantity('reserved')}</strong><small>已确认订单锁定</small></div>
    <i>−</i>
    <div className="inventory-equation-item quarantined"><span>退货隔离</span><strong>{quantity('quarantined')}</strong><small>等待质检处置</small></div>
    <i>=</i>
    <div className="inventory-equation-item available"><span>可用库存</span><strong>{quantity('available')}</strong><small>当前可承诺数量</small></div>
    <div className="inventory-equation-value"><span>库存账面值</span><strong>{currencies.size ? [...currencies].sort(([a], [b]) => a.localeCompare(b)).map(([currency, amount]) => <span className="inventory-summary-amount" key={currency}>{formatMoney(currency, amount)}</span>) : '—'}</strong><small>{unknownCurrency ? `${unknownCurrency} 条余额币种缺失或混用，未计入金额汇总，请核对库存流水。` : '按流水币种分别汇总，不跨币种相加'}</small></div>
  </section>;
}
