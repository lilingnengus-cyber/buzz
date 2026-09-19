import React from "react";
import {
  request,
  toApiFailure,
  type ApiFailure,
  type SalesOrder,
  type PurchaseOrder,
} from "./api";
import { RecordDetail } from "./OrderWorkflowModal";
import {
  salesOrderDetail,
  purchaseOrderDetail,
  type RecordDetailAction,
} from "./OrderWorkflowRecordDetails";
import { PageLoadFailure } from "./PageLoadFailure";

export function LinkedOrderDetail({
  domain,
  id,
}: {
  domain: "sales" | "purchase";
  id: string;
}) {
  const [detail, setDetail] = React.useState<RecordDetailAction | null>(null);
  const [failure, setFailure] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  const label = domain === "sales" ? "销售订单" : "采购订单";
  React.useEffect(() => {
    let active = true;
    setDetail(null);
    setFailure(null);
    const load =
      domain === "sales"
        ? request<SalesOrder>(
            `/api/v1/sales-orders/${encodeURIComponent(id)}`,
          ).then(salesOrderDetail)
        : request<PurchaseOrder>(
            `/api/v1/purchase-orders/${encodeURIComponent(id)}`,
          ).then(purchaseOrderDetail);
    void load
      .then((value) => {
        if (active) setDetail(value);
      })
      .catch((error: unknown) => {
        if (active) setFailure(toApiFailure(error));
      });
    return () => {
      active = false;
    };
  }, [domain, id, revision]);
  return (
    <section className="workflow-page">
      <h1>{detail?.title ?? `${label}详情`}</h1>
      {failure ? (
        <PageLoadFailure
          failure={failure}
          resourceLabel={label}
          onRetry={() => setRevision((value) => value + 1)}
        />
      ) : detail ? (
        <RecordDetail state={detail} />
      ) : (
        <p role="status">正在加载{label}…</p>
      )}
    </section>
  );
}
