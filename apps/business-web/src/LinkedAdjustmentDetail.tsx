import React from "react";
import { request, toApiFailure, type ApiFailure } from "./api";
import { formatMoney } from "./formatters";
import { PageLoadFailure } from "./PageLoadFailure";
import { AuthorityAssignmentPair } from "./AuthorityAssignmentPair";

type Line = {
  id: string; metric_type: string; amount: string; currency: string;
  business_date: string; allocation_basis: string; reason_code: string;
  direct_sales_order_id: string | null; source_ref?: string | null;
};
type Detail = {
  schemaVersion: number;
  batch: { id: string; adjustment_number: string; legal_entity_id: string; status: string; management_period: string; currency: string; version: number };
  businessUnitIds: string[];
  lines: Line[]; totalAmount: string; targetOrderCount: number; version: number;
  boundary: string;
  pagination: { offset: number; total: number; nextOffset: number | null };
};
const labels: Record<string, string> = {
  draft: "草稿", previewed: "已预览", posted: "已过账", reversed: "已逆转",
  outbound_freight: "销售运费", sales_commission: "销售佣金", platform_fee: "平台费用",
  customer_rebate: "客户返利", supplier_rebate: "供应商返利", other_direct_cost: "其他直接成本",
  allocated_operating_expense: "分摊经营费用", direct: "直接归集", net_revenue: "按净收入",
  product_cost: "按商品成本", shipped_quantity: "按出库数量", fixed_weight: "固定权重",
};
export function LinkedAdjustmentDetail({ id }: { id: string }) {
  const [detail, setDetail] = React.useState<Detail | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, retry] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setDetail(null); setError(null);
    async function load() {
      let offset = 0;
      let first: Detail | null = null;
      const lines: Line[] = [];
      const ids = new Set<string>();
      while (true) {
        const query = new URLSearchParams({ offset: String(offset), limit: "100" });
        if (first) query.set("expectedVersion", String(first.version));
        const page = await request<Detail>(`/api/v1/profit-adjustments/${encodeURIComponent(id)}?${query}`);
        if (!active) return;
        if (page.schemaVersion !== 1 || page.batch.id !== id || page.batch.version !== page.version
          || page.boundary !== "management_only_not_general_ledger" || page.pagination.offset !== offset
          || !Number.isSafeInteger(page.pagination.total) || page.pagination.total < 0
          || (first && (page.version !== first.version || page.totalAmount !== first.totalAmount
            || page.pagination.total !== first.pagination.total
            || JSON.stringify(page.businessUnitIds) !== JSON.stringify(first.businessUnitIds)))) throw new Error("费用明细已变化，请重新读取。");
        first ??= page;
        for (const line of page.lines) {
          if (ids.has(line.id)) throw new Error("费用明细重复，请重新读取。");
          ids.add(line.id); lines.push(line);
        }
        const next = page.pagination.nextOffset;
        if (next === null) {
          if (lines.length !== page.pagination.total) throw new Error("费用明细不完整，请重新读取。");
          setDetail({ ...first, lines }); return;
        }
        if (!Number.isSafeInteger(next) || next !== offset + page.lines.length || next <= offset
          || next >= page.pagination.total) throw new Error("费用明细分页无效，请重新读取。");
        offset = next;
      }
    }
    void load().catch((cause: unknown) => { if (active) setError(toApiFailure(cause)); });
    return () => { active = false; };
  }, [id, revision]);
  const prefix = window.location.pathname.startsWith("/embed/") ? "/embed" : "";
  return <section className="workflow-page" data-testid="adjustment-detail">
    <h1>经营费用{detail ? ` · ${detail.batch.adjustment_number}` : "详情"}</h1>
    <p>经营管理口径。过账记录费用，逆转保留原事实并追加抵销记录，不发起银行退款。</p>
    {error ? <PageLoadFailure failure={error} resourceLabel="经营费用" onRetry={() => retry(n => n + 1)} />
      : !detail ? <p>正在读取完整费用明细…</p> : <>
        <p>{labels[detail.batch.status] ?? detail.batch.status} · 期间 {detail.batch.management_period} · 版本 {detail.version}</p>
        <AuthorityAssignmentPair legalEntityId={detail.batch.legal_entity_id} businessUnitIds={detail.businessUnitIds} businessUnitFallback="未指定经营单元" />
        <p>费用合计 {formatMoney(detail.batch.currency, detail.totalAmount)} · {detail.lines.length} 条明细 · {detail.targetOrderCount} 个目标订单</p>
        <table><thead><tr><th>日期</th><th>费用类型</th><th>金额</th><th>分摊方式</th><th>原因</th><th>来源</th><th>直接归集订单</th></tr></thead>
          <tbody>{detail.lines.map(line => <tr key={line.id}>
            <td>{line.business_date}</td><td>{labels[line.metric_type] ?? line.metric_type}</td>
            <td>{formatMoney(line.currency, line.amount)}</td><td>{labels[line.allocation_basis] ?? line.allocation_basis}</td>
            <td>{line.reason_code}</td><td>{line.source_ref || "—"}</td>
            <td>{line.direct_sales_order_id ? <a href={`${prefix}/sales-orders/${encodeURIComponent(line.direct_sales_order_id)}`}>查看订单</a> : "—"}</td>
          </tr>)}</tbody></table>
      </>}
  </section>;
}
