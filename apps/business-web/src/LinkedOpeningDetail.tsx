import React from "react";
import { formatAmount, formatQuantity } from "./formatters";
import { request, toApiFailure, type ApiFailure } from "./api";
import { PageLoadFailure } from "./PageLoadFailure";

type Opening = {
  number: string;
  status: string;
  businessDate: string;
  currency: string;
  version: number;
  lines: Array<{
    warehouseName: string;
    skuName: string;
    quantity: string;
    unitCost: string;
    totalCost: string;
  }>;
};

export function LinkedOpeningDetail({ id }: { id: string }) {
  const [item, setItem] = React.useState<Opening | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setItem(null);
    setError(null);
    void request<Opening>(
      `/api/v1/inventory-openings/${encodeURIComponent(id)}`,
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
      <h1>期初库存{item ? ` · ${item.number}` : "详情"}</h1>
      {error ? (
        <PageLoadFailure
          failure={error}
          resourceLabel="期初库存"
          onRetry={() => setRevision((value) => value + 1)}
        />
      ) : item ? (
        <>
          <p>
            {item.businessDate} · {item.currency} ·{" "}
            {(
              { draft: "草稿", posted: "已过账", reversed: "已冲销" } as Record<
                string,
                string
              >
            )[item.status] ?? item.status}{" "}
            · 版本 {item.version}
          </p>
          <table>
            <thead>
              <tr>
                <th>仓库</th>
                <th>商品</th>
                <th>数量</th>
                <th>单位成本</th>
                <th>成本合计</th>
              </tr>
            </thead>
            <tbody>
              {item.lines.map((line, index) => (
                <tr key={`${line.warehouseName}-${line.skuName}-${index}`}>
                  <td>{line.warehouseName}</td>
                  <td>{line.skuName}</td>
                  <td>{formatQuantity(line.quantity)}</td>
                  <td>{formatAmount(line.unitCost)}</td>
                  <td>{formatAmount(line.totalCost)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p>只读详情。草稿尚未增加库存；过账后登记库存数量与成本。</p>
        </>
      ) : (
        <p role="status">正在加载期初库存…</p>
      )}
    </section>
  );
}
