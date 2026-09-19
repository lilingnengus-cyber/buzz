import { formatAmount, formatQuantity } from "./formatters";

export type ReversalRecord = {
  date: string;
  reason: string;
  version: number;
  financial: {
    originalAmountBefore: string;
    originalAmountAfter: string;
    openAmountBefore: string;
    openAmountAfter: string;
    settledAmount: string;
  };
  inventory: Array<{
    skuId: string;
    onHandQuantityBefore: string;
    onHandQuantityAfter: string;
    quarantinedQuantityBefore: string;
    quarantinedQuantityAfter: string;
    inventoryValueBefore: string;
    inventoryValueAfter: string;
  }>;
};

export function ReturnReversalRecord({ record, side, currency, lines }: {
  record: ReversalRecord;
  side: "sales" | "purchase";
  currency: string;
  lines: Array<{ skuId: string; skuName: string; skuCode: string }>;
}) {
  const finance = record.financial;
  const label = side === "sales" ? "应收" : "应付";
  return (
    <section aria-label="冲销记录">
      <h2>冲销记录</h2>
      <p>冲销日期：{record.date} · 版本 {record.version}</p>
      <p>冲销原因：{record.reason}</p>
      <p>以下为本次冲销发生时的变化，金额币种为 {currency}。</p>
      <p>{label}原额：{formatAmount(finance.originalAmountBefore)} → {formatAmount(finance.originalAmountAfter)}</p>
      <p>{label}未结余额：{formatAmount(finance.openAmountBefore)} → {formatAmount(finance.openAmountAfter)}</p>
      <p>已结金额保持不变：{formatAmount(finance.settledAmount)}</p>
      <table aria-label="冲销库存变化">
        <thead><tr><th>商品</th><th>库存数量（前 → 后）</th><th>隔离数量（前 → 后）</th><th>库存价值（前 → 后）</th></tr></thead>
        <tbody>{record.inventory.map((effect) => {
          const sku = lines.find((line) => line.skuId === effect.skuId);
          return <tr key={effect.skuId}>
            <td>{sku ? `${sku.skuCode} · ${sku.skuName}` : effect.skuId}</td>
            <td>{formatQuantity(effect.onHandQuantityBefore)} → {formatQuantity(effect.onHandQuantityAfter)}</td>
            <td>{formatQuantity(effect.quarantinedQuantityBefore)} → {formatQuantity(effect.quarantinedQuantityAfter)}</td>
            <td>{formatAmount(effect.inventoryValueBefore)} → {formatAmount(effect.inventoryValueAfter)}</td>
          </tr>;
        })}</tbody>
      </table>
    </section>
  );
}
