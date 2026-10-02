import React from "react";
import { request, type Envelope, type SalesOrder } from "./api";
import { formatMoney } from "./formatters";
const statuses: Record<string, string> = {
  draft: "草稿",
  confirmed: "已确认",
  completed: "已完成",
  closed: "已关闭",
  cancelled: "已取消",
};

export function CrmRelatedOrders({
  opportunityId,
  revision,
}: {
  opportunityId: string;
  revision: number;
}) {
  const [items, setItems] = React.useState<SalesOrder[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [retry, setRetry] = React.useState(0);
  React.useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError("");
    request<Envelope<SalesOrder>>(
      `/api/v1/sales-orders?opportunityId=${encodeURIComponent(opportunityId)}&limit=200`,
      { signal: controller.signal },
    )
      .then((data) => {
        if (!controller.signal.aborted) setItems(data.items);
      })
      .catch((reason) => {
        if (!controller.signal.aborted)
          setError(reason instanceof Error ? reason.message : "订单读取失败");
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [opportunityId, revision, retry]);
  return (
    <section className="crm-related-orders">
      <h3>关联销售订单</h3>
      {loading ? (
        <p role="status" className="crm-hint">
          正在读取订单…
        </p>
      ) : error ? (
        <div role="alert" className="crm-error">
          <p>{error}</p>
          <button onClick={() => setRetry((v) => v + 1)}>重新加载订单</button>
        </div>
      ) : items.length ? (
        <>
          {items.map((o) => (
            <a
              className="crm-related-order"
              key={o.id}
              href={`/sales/orders/${encodeURIComponent(o.id)}`}
            >
              <div>
                <strong>{o.orderNumber}</strong>
                <span className="crm-hint">{o.orderDate}</span>
              </div>
              <span>{formatMoney(o.currency, Number(o.grossAmount))}</span>
              <span className="crm-stage">
                {statuses[o.lifecycleStatus] || o.lifecycleStatus}
              </span>
            </a>
          ))}
          {items.length === 200 && (
            <p className="crm-hint">显示最近 200 笔关联订单。</p>
          )}
        </>
      ) : (
        <p className="crm-hint">
          尚无可查看的关联订单。通过此商机创建的销售订单会显示在这里。
        </p>
      )}
    </section>
  );
}
