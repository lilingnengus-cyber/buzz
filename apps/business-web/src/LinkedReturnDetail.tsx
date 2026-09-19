import React from "react";
import { ReturnReversalRecord, type ReversalRecord } from "./ReturnReversalRecord";
import { request, toApiFailure, type ApiFailure } from "./api";
import { formatAmount, formatQuantity } from "./formatters";
import { returnReason, statusLabel } from "./OrderWorkflowRecordDetails";
import { PageLoadFailure } from "./PageLoadFailure";

type ReturnDetail = {
  id: string;
  number: string;
  sourceId: string;
  status: string;
  workflowStatus: string;
  businessDate: string;
  currency: string;
  version: number;
  reasonCode: string;
  businessNote: string | null;
  amount: string;
  cost: string;
  reversal?: ReversalRecord | null;
  lines: Array<{
    skuId: string;
    returnLineId: string;
    skuCode: string;
    skuName: string;
    quantity: string;
    unitCost: string;
    totalCost: string;
  }>;
};

export function LinkedReturnDetail({
  side,
  id,
}: {
  side: "sales" | "purchase";
  id: string;
}) {
  const [item, setItem] = React.useState<ReturnDetail | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  const title = side === "sales" ? "销售退货" : "采购退货";
  React.useEffect(() => {
    let active = true;
    setItem(null);
    setError(null);
    void request<ReturnDetail>(
      `/api/v1/${side}-returns/${encodeURIComponent(id)}`,
    )
      .then((value) => {
        if (active) setItem(value);
      })
      .catch((failure: unknown) => {
        if (active) setError(toApiFailure(failure));
      });
    return () => {
      active = false;
    };
  }, [side, id, revision]);
  return (
    <section className="workflow-page">
      <h1>
        {title}
        {item ? ` · ${item.number}` : "详情"}
      </h1>
      {error ? (
        <PageLoadFailure
          failure={error}
          resourceLabel={title}
          onRetry={() => setRevision((value) => value + 1)}
        />
      ) : item ? (
        <>
          <p>
            {item.businessDate} · {item.currency} ·{" "}
            {item.status === "cancelled" ? "已取消" : statusLabel(item.status)}{" "}
            · {item.status === "reversed" ? "冲销前处置状态：" : ""}{statusLabel(item.workflowStatus)} · 版本 {item.version}
          </p>
          <p>退货原因：{returnReason(item.reasonCode)}</p>
          {item.businessNote && <p>备注：{item.businessNote}</p>}
          <p>
            退货金额：{formatAmount(item.amount)} · 成本：
            {formatAmount(item.cost)}
          </p>
          {item.reversal && <ReturnReversalRecord record={item.reversal} side={side} currency={item.currency} lines={item.lines} />}
          <table>
            <thead>
              <tr>
                <th>商品编码</th>
                <th>商品</th>
                <th>退货数量</th>
                <th>单位成本</th>
                <th>成本合计</th>
              </tr>
            </thead>
            <tbody>
              {item.lines.map((line) => (
                <tr key={line.returnLineId}>
                  <td>{line.skuCode}</td>
                  <td>{line.skuName}</td>
                  <td>{formatQuantity(line.quantity)}</td>
                  <td>{formatAmount(line.unitCost)}</td>
                  <td>{formatAmount(line.totalCost)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p>
            <a
              href={`/embed/${side === "sales" ? "shipments" : "goods-receipts"}/${encodeURIComponent(item.sourceId)}`}
            >
              查看关联{side === "sales" ? "出库" : "收货"}单
            </a>
          </p>
          <p>
            {item.status === "reversed" ? "原退货与冲销记录均已保留；冲销不代表实际退款或物流操作。" : item.status === "draft"
              ? "草稿尚未改变库存和应收应付；采购退货的最终成本在确认时确定。"
              : "退货确认不代表已经退款或完成后续质检、发运与签收，请核对处置状态。"}
          </p>
        </>
      ) : (
        <p role="status">正在加载{title}…</p>
      )}
    </section>
  );
}
