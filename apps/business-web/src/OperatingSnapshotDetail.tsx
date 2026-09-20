import React from "react";
import {
  type ApiFailure,
  type OperatingTrendSnapshot,
  request,
  toApiFailure,
} from "./api";
import { formatMoney, formatDecimal } from "./formatters";
import { PageLoadFailure } from "./PageLoadFailure";

type Detail = Omit<OperatingTrendSnapshot, "change"> & {
  ownerUserId: string;
  scope: Record<string, string[]> | null;
  scopeBasis: "recorded" | "legacy_current_identity";
};
export function OperatingSnapshotDetail({ id }: { id: string }) {
  const [detail, setDetail] = React.useState<Detail | null>(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, retry] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setDetail(null);
    setError(null);
    request<Detail>(`/api/v1/operations/snapshots/${encodeURIComponent(id)}`)
      .then((value) => {
        if (active) setDetail(value);
      })
      .catch((cause: unknown) => {
        if (active) setError(toApiFailure(cause));
      });
    return () => {
      active = false;
    };
  }, [id, revision]);
  const prefix = window.location.pathname.startsWith("/embed/") ? "/embed" : "";
  return (
    <main
      className="operating-trend-page"
      data-testid="operating-snapshot-detail"
    >
      <header className="page-head">
        <div>
          <span className="eyebrow">不可变经营快照</span>
          <h1>
            {detail?.cadence === "weekly"
              ? "经营周报"
              : detail
                ? "经营日报"
                : "经营快照"}
          </h1>
          <p>查看生成时冻结的内容。当前业务变化不会改写这份报表。</p>
        </div>
        <a href={`${prefix}/operating-trends`}>返回日报与趋势</a>
      </header>
      {error && (
        <PageLoadFailure
          failure={error}
          resourceLabel="经营快照"
          onRetry={() => retry((n) => n + 1)}
        />
      )}
      {!error && !detail && <p className="empty">正在读取经营快照…</p>}
      {detail && (
        <>
          <div className="balance-callout">
            <span>
              {detail.periodStart} 至 {detail.periodEnd}（结束日期不含） ·{" "}
              {offset(detail.utcOffsetMinutes)}
            </span>
            <strong>
              {formatMoney(
                detail.currency,
                detail.metrics.managementOperatingProfit,
              )}
            </strong>
            <small>
              管理经营利润 · 数据质量：
              {
                { complete: "完整", partial: "部分完整", blocked: "存在差异" }[
                  detail.dataQualityStatus
                ]
              }
            </small>
          </div>
          <table>
            <thead>
              <tr>
                <th>经营指标</th>
                <th>冻结值</th>
              </tr>
            </thead>
            <tbody>
              {(
                [
                  ["销售订单数", String(detail.metrics.salesOrderCount)],
                  [
                    "销售订单金额",
                    formatMoney(
                      detail.currency,
                      detail.metrics.salesOrderAmount,
                    ),
                  ],
                  ["出库单数", String(detail.metrics.shipmentCount)],
                  [
                    "出库收入",
                    formatMoney(detail.currency, detail.metrics.shippedRevenue),
                  ],
                  ["采购订单数", String(detail.metrics.purchaseOrderCount)],
                  [
                    "采购订单金额",
                    formatMoney(
                      detail.currency,
                      detail.metrics.purchaseOrderAmount,
                    ),
                  ],
                  [
                    "生成时点库存价值",
                    formatMoney(
                      detail.currency,
                      detail.metrics.inventoryValueAsOfGeneration,
                    ),
                  ],
                  [
                    "生成时点缺货数",
                    String(detail.metrics.stockoutCountAsOfGeneration),
                  ],
                  [
                    "新增异常",
                    detail.metrics.incidentsOpened == null
                      ? "不可按法人拆分"
                      : String(detail.metrics.incidentsOpened),
                  ],
                  [
                    "已解决异常",
                    detail.metrics.incidentsResolved == null
                      ? "不可按法人拆分"
                      : String(detail.metrics.incidentsResolved),
                  ],
                  [
                    "SLA 超时数",
                    detail.metrics.slaBreached == null
                      ? "不可按法人拆分"
                      : String(detail.metrics.slaBreached),
                  ],
                  [
                    "平均解决时长（小时）",
                    detail.metrics.averageResolutionHours == null
                      ? "不可按法人拆分"
                      : formatDecimal(detail.metrics.averageResolutionHours),
                  ],
                ] as const
              ).map(([label, value]) => (
                <tr key={label}>
                  <th scope="row">{label}</th>
                  <td>{value}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p>
            生成时间：
            {new Date(detail.generatedAt).toLocaleString("zh-CN", {
              timeZone: "Asia/Shanghai",
            })}
            （UTC+08:00）
          </p>
          <p className="report-warning">
            库存为生成时点值，其余指标按报表周期统计。本报表为经营管理口径，不是法定财务报表。
          </p>
          <details>
            <summary>查看冻结范围与来源</summary>
            {detail.scope ? (
              <dl>
                {Object.entries(detail.scope).map(([key, ids]) => (
                  <React.Fragment key={key}>
                    <dt>{scopeNames[key] ?? key}</dt>
                    <dd>{ids.length ? ids.join("、") : "无已授权记录"}</dd>
                  </React.Fragment>
                ))}
              </dl>
            ) : (
              <p>旧快照未保存历史范围，仅按原授权身份验证访问。</p>
            )}
            <p>
              快照编号：<code>{detail.id}</code>
            </p>
            <p>
              来源摘要：<code>{detail.sourceHash}</code>
            </p>
          </details>
        </>
      )}
    </main>
  );
}
const scopeNames: Record<string, string> = {
  legalEntityIds: "法人",
  customerIds: "客户",
  supplierIds: "供应商",
  brandIds: "品牌",
  businessUnitIds: "业务单元",
  warehouseIds: "仓库",
};
function offset(minutes?: number | null) {
  if (minutes == null) return "时区未知";
  const n = Math.abs(minutes);
  return `UTC${minutes >= 0 ? "+" : "-"}${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}`;
}
