import React from "react";
import {
  request,
  toApiFailure,
  type ApiFailure,
  type InventoryCountDetail,
} from "./api";
import { formatAmount, formatQuantity } from "./formatters";
import { PageLoadFailure } from "./PageLoadFailure";

const STATES: Record<string, string> = {
  counting: "待录入实盘",
  counted: "待确认差异",
  posted: "已过账",
  cancelled: "已取消",
};

export function LinkedInventoryCountDetail({ id }: { id: string }) {
  const [item, setItem] = React.useState<InventoryCountDetail | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setItem(null);
    setError(null);
    void request<InventoryCountDetail>(
      `/api/v1/inventory-counts/${encodeURIComponent(id)}`,
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
  }, [id, revision]);
  return (
    <section className="workflow-page">
      <h1>库存盘点{item ? ` · ${item.countNumber}` : "详情"}</h1>
      {error ? (
        <PageLoadFailure
          failure={error}
          resourceLabel="库存盘点"
          onRetry={() => setRevision((value) => value + 1)}
        />
      ) : item ? (
        <>
          <p>
            {item.countDate} · {item.currency} ·{" "}
            {STATES[item.status] ?? item.status} · 版本 {item.version}
          </p>
          <p>
            {item.status === "counting" || item.status === "counted"
              ? "所选仓库与商品仍在冻结中，过账或取消后解除冻结。"
              : item.status === "posted"
                ? "盘点差异已登记库存，当前盘点的冻结已解除。"
                : item.status === "cancelled"
                  ? "盘点已取消，未登记库存差异，当前盘点的冻结已解除。"
                  : ""}
          </p>
          <table>
            <thead>
              <tr>
                <th>商品</th>
                <th>账面数量</th>
                <th>预留数量</th>
                <th>隔离数量</th>
                <th>实盘数量</th>
                <th>差异数量</th>
                <th>差异金额（{item.currency}）</th>
              </tr>
            </thead>
            <tbody>
              {item.lines.map((line) => (
                <tr key={line.id}>
                  <td>
                    {line.skuCode} · {line.skuName}
                  </td>
                  <td>{formatQuantity(line.snapshotOnHandQuantity)}</td>
                  <td>{formatQuantity(line.snapshotReservedQuantity)}</td>
                  <td>{formatQuantity(line.snapshotQuarantinedQuantity)}</td>
                  <td>
                    {line.actualOnHandQuantity === null
                      ? "未录入"
                      : formatQuantity(line.actualOnHandQuantity)}
                  </td>
                  <td>
                    {line.varianceQuantity === null
                      ? "—"
                      : formatQuantity(line.varianceQuantity)}
                  </td>
                  <td>
                    {line.varianceValue === null
                      ? "—"
                      : formatAmount(line.varianceValue)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      ) : (
        <p role="status">正在加载库存盘点…</p>
      )}
    </section>
  );
}
