import React from "react";
import { request, type ShipmentDraftOptions } from "./api";
import { formatMoney, formatQuantity } from "./formatters";

type Row = {
  id: string;
  order: string;
  orderId?: string;
  customer: string;
  title: string;
  value: string;
};
type ServiceOptions = {
  orderLines?: {
    id: string;
    order_number: string;
    customer_name: string;
    title: string;
    amount: string;
    currency: string;
  }[];
  hasMoreOrders?: boolean;
};
export function FulfillmentQueue({
  service = false,
  revision,
}: {
  service?: boolean;
  revision: number;
}) {
  const [rows, setRows] = React.useState<Row[]>([]);
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(true);
  const [more, setMore] = React.useState(false);
  React.useEffect(() => {
    let active = true;
    setBusy(true);
    setError("");
    const load = async () => {
      if (service) {
        const d = await request<ServiceOptions>(
          "/api/v1/service-project-options",
        );
        return {
          rows: (d.orderLines ?? []).map((r) => ({
            id: r.id,
            order: r.order_number,
            customer: r.customer_name,
            title: r.title,
            value: formatMoney(r.currency, r.amount),
          })),
          more: !!d.hasMoreOrders,
        };
      }
      const d = await request<ShipmentDraftOptions>(
        "/api/v1/shipments/draft-options?limit=500",
      );
      return {
        rows: d.items.map((r) => ({
          id: r.salesOrderLineId,
          order: r.orderNumber,
          orderId: r.orderId,
          customer: r.customerName,
          title: r.skuName,
          value: `可出库 ${formatQuantity(r.shippableQuantity)}`,
        })),
        more: d.items.length >= 500,
      };
    };
    load()
      .then((d) => {
        if (active) {
          setRows(d.rows);
          setMore(d.more);
        }
      })
      .catch((e) => active && setError(e.message))
      .finally(() => active && setBusy(false));
    return () => {
      active = false;
    };
  }, [service, revision]);
  return (
    <details className="fulfillment-queue">
      <summary>
        {service ? "待创建项目的服务订单行" : "待出库的商品订单行"}
        {!busy && !error ? ` · ${rows.length} 条` : ""}
      </summary>
      {busy ? (
        <p>正在读取…</p>
      ) : error ? (
        <p role="alert">{error}</p>
      ) : (
        <>
          <p>
            {service
              ? "已确认且尚未关联项目的服务行。新建服务项目时选择对应订单行。"
              : "仅显示已确认、尚有可出库数量的商品行。新建出库单时选择对应订单。"}
          </p>
          {rows.length === 0 ? (
            <p>暂无待处理订单行</p>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>订单</th>
                  <th>客户</th>
                  <th>{service ? "服务" : "商品"}</th>
                  <th>{service ? "含税金额" : "待出库数量"}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.id}>
                    <td>
                      {r.orderId ? (
                        <a href={`/sales/orders/${r.orderId}`}>{r.order}</a>
                      ) : (
                        r.order
                      )}
                    </td>
                    <td>{r.customer}</td>
                    <td>{r.title}</td>
                    <td>{r.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {more && <p>候选较多，更多记录请在新建表单中选择。</p>}
        </>
      )}
    </details>
  );
}
