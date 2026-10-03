import React from "react";
import { request } from "./api";
import { CrmRelatedOrders } from "./CrmRelatedOrders";
import { WorkflowPage } from "./OrderWorkflowLayout";
import { PageLoadFailure } from "./PageLoadFailure";
import { toApiFailure, type ApiFailure } from "./api";

/** Read-only workflow focused on orders visible for one opportunity. */
export function OpportunityWorkflow({ id, mode }: { id: string; mode: "goods" | "service" }) {
  const [title, setTitle] = React.useState("");
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    const controller = new AbortController();
    setTitle("");
    setError(null);
    request<{ item: { title: string } }>(`/api/v1/crm/opportunities/${encodeURIComponent(id)}`, { signal: controller.signal })
      .then((data) => { if (!controller.signal.aborted) setTitle(data.item.title); })
      .catch((reason) => { if (!controller.signal.aborted) setError(toApiFailure(reason)); });
    return () => controller.abort();
  }, [id, revision]);
  const section = mode === "goods" ? "goodsOrders" : "serviceOrders";
  return <WorkflowPage primaryAction={undefined} secondaryAction={undefined} domain="sales" eyebrow="业务闭环" title={mode === "goods" ? "商品订单闭环" : "服务订单闭环"} caption="查看当前商机关联订单的履约与回款。">
    <div className="crm-heading">
      <p>商机筛选：{title || "正在读取…"}</p>
      <a href={`/#${section}`}>清除商机筛选</a>
    </div>
    <p><a href={`/#crm?opportunity=${encodeURIComponent(id)}`}>返回商机</a></p>
    {error ? <PageLoadFailure failure={error} resourceLabel="商机" onRetry={() => setRevision((v) => v + 1)} /> : title ? <>
      <p className="crm-hint">仅显示当前可查看的关联订单。回款汇总包含订单全部有效应收；商品与服务共用订单回款。</p>
      <CrmRelatedOrders opportunityId={id} revision={revision} workflowMode={mode} />
    </> : <p role="status">正在读取商机…</p>}
  </WorkflowPage>;
}
