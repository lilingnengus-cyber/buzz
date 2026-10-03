import React from "react";
import { request, type Envelope, type SalesOrder } from "./api";
import { formatMoney } from "./formatters";
import { SalesOrderProgress } from "./SalesOrderProgress";
import "./order-workflow-detail.css";
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
  workflowMode,
}: {
  opportunityId: string;
  revision: number;
  workflowMode?: "goods" | "service";
}) {
  const [items, setItems] = React.useState<SalesOrder[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [retry, setRetry] = React.useState(0);
  React.useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setItems([]);
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
      <header className="crm-heading">
        <h3>关联销售订单</h3>
        <button type="button" disabled={loading} onClick={() => setRetry((v) => v + 1)}>
          刷新关联订单
        </button>
      </header>
      {!workflowMode && items.length > 0 && !loading && !error && (
        <p className="crm-workflow-links">
          <a href={`/#goodsOrders?opportunity=${encodeURIComponent(opportunityId)}`}>查看此商机商品订单闭环</a>{" · "}
          <a href={`/#serviceOrders?opportunity=${encodeURIComponent(opportunityId)}`}>查看此商机服务订单闭环</a>
        </p>
      )}
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
            <RelatedOrder key={o.id} order={o} workflowMode={workflowMode} />
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

function RelatedOrder({ order, workflowMode }: { order: SalesOrder; workflowMode?: "goods" | "service" }) {
  const [expanded, setExpanded] = React.useState(false);
  return (
    <article className="crm-related-order-card" aria-label={`关联订单 ${order.orderNumber}`}>
      <a className="crm-related-order" href={`/sales/orders/${encodeURIComponent(order.id)}`}>
        <div>
          <strong>{order.orderNumber}</strong>
          <span className="crm-hint">{order.orderDate}</span>
        </div>
        <span>{formatMoney(order.currency, order.grossAmount)}</span>
        <span className="crm-stage">{statuses[order.lifecycleStatus] || order.lifecycleStatus}</span>
      </a>
      <details open={expanded} onToggle={(event) => setExpanded(event.currentTarget.open)}>
        <summary>履约、验收与回款汇总</summary>
        {expanded && <SalesOrderProgress id={order.id} summaryOnly={!workflowMode} fulfillmentKind={workflowMode} />}
      </details>
    </article>
  );
}
