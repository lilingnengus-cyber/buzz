import React from "react";
import { request, toApiFailure, type ApiFailure } from "./api";
import { formatMoney, formatQuantity } from "./formatters";
import { PageLoadFailure } from "./PageLoadFailure";

type Line = {
  lineNumber: number;
  skuCode: string;
  name: string;
  unit: string;
  ordered: string;
  delivered: string;
  cancelled: string;
  remaining: string;
  complete: boolean;
  projectTitle: string | null;
  projectStatus: string | null;
};
export type Progress = {
  goods: Line[] | null;
  services: Line[] | null;
  payment: { receivableCount: number; amount: string; settled: string; open: string; overdue: string } | null;
  dataAsOf: string;
};
const projectStatus: Record<string, string> = {
  pending: "待开始", paused: "已暂停", draft: "草稿", active: "交付中", delivery: "交付中", acceptance: "待验收", completed: "已完成", cancelled: "已取消",
};

export type SalesProgressSnapshot = { currency: string; progress: Progress };

export function SalesOrderProgress({ id, initial, summaryOnly = false }: { id: string; initial?: SalesProgressSnapshot; summaryOnly?: boolean }) {
  const [data, setData] = React.useState<{ currency: string; progress: Progress } | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, refresh] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setData(null);
    setError(null);
    if (revision === 0 && initial) { setData(initial); return; }
    void request<{ currency: string; progress: Progress }>(`/api/v1/sales-orders/${encodeURIComponent(id)}`)
      .then((value) => {
        if (!value.progress) throw new Error("暂未获取到订单进度，请刷新重试");
        if (active) setData(value);
      })
      .catch((reason: unknown) => { if (active) setError(toApiFailure(reason)); });
    return () => { active = false; };
  }, [id, revision, initial]);
  return <section className="sales-order-progress" aria-label="履约与回款进度">
    <header><h3>履约与回款进度</h3><button type="button" onClick={() => refresh(v => v + 1)}>刷新进度</button></header>
    {error ? <PageLoadFailure failure={error} resourceLabel="订单进度" onRetry={() => refresh(v => v + 1)} /> : !data ? <p role="status">正在读取订单进度…</p> : <>
      <LineProgress title="商品交付" lines={data.progress.goods} summaryOnly={summaryOnly} />
      <LineProgress title="服务验收" lines={data.progress.services} service summaryOnly={summaryOnly} />
      <section aria-label="回款进度"><h4>回款进度</h4>
        {data.progress.payment === null ? <p>当前权限无法查看回款进度。</p> : <>
          <dl className="record-detail-grid">
            {([['有效应收', data.progress.payment.amount], ['已核销回款', data.progress.payment.settled], ['待收金额', data.progress.payment.open], ['其中逾期', data.progress.payment.overdue]] as const).map(([label, amount]) => <div className="record-detail-field format-money" key={label}><dt>{label}</dt><dd>{formatMoney(data.currency, amount)}</dd></div>)}
          </dl>
          {data.progress.payment.receivableCount === 0 && <p>尚未形成有效应收。</p>}
          <p>仅统计本订单有效应收及已核销回款，已撤销应收不计入；客户未分配收款不计入本订单回款。有效应收按退货调整后的余额计算，不代表全部订单金额。</p>
        </>}
      </section>
      <p>商品数量按确认出库及出库撤销计算，退货另在商品订单闭环查看。更新于 {new Date(data.progress.dataAsOf).toLocaleString("zh-CN")}</p>
    </>}
  </section>;
}

function LineProgress({ title, lines, service = false, summaryOnly = false }: { title: string; lines: Line[] | null; service?: boolean; summaryOnly?: boolean }) {
  return <section aria-label={title}><h4>{title}</h4>
    {lines === null ? <p>当前权限或数据范围不足，无法查看完整{title}进度。</p> : lines.length === 0 ? <p>此订单没有{service ? '服务' : '商品'}行。</p> : <>
      <p>已完成 {lines.filter(line => line.complete).length} / {lines.length} 行 · 全部取消 {lines.filter(line => Number(line.cancelled) === Number(line.ordered)).length} 行</p>
      {!summaryOnly && <div className="order-progress-table"><table><thead><tr><th>明细</th><th>订购</th><th>{service ? '已验收' : '已出库'}</th><th>已取消</th><th>待{service ? '验收' : '出库'}</th>{service && <th>服务项目</th>}</tr></thead><tbody>
        {lines.map(line => <tr key={line.lineNumber}><td>{line.name}<small>{line.skuCode} · {line.unit}</small></td><td>{formatQuantity(line.ordered)}</td><td>{formatQuantity(line.delivered)}</td><td>{formatQuantity(line.cancelled)}</td><td>{formatQuantity(line.remaining)}</td>{service && <td>{line.projectTitle ?? '未关联项目'}{line.projectStatus && <small>{projectStatus[line.projectStatus] ?? line.projectStatus}</small>}</td>}</tr>)}
      </tbody></table></div>}
    </>}
  </section>;
}
