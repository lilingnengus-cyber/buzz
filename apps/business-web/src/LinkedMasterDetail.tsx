import React from "react";
import {
  request,
  toApiFailure,
  type ApiFailure,
  type CoreMasterRecord,
  type ProductMasterRecord,
} from "./api";
import { PageLoadFailure } from "./PageLoadFailure";
import { formatMoney } from "./formatters";
import { masterDetailRoute } from "./masterDetailRoute";

const FIELDS: Array<[string, string]> = [
  ["legalEntityName", "法定主体"],
  ["businessUnitName", "业务单元"],
  ["countryCode", "国家/地区"],
  ["functionalCurrency", "本位币"],
  ["registrationNumber", "登记号"],
  ["address", "地址"],
  ["paymentTermsDays", "账期（天）"],
  ["productName", "所属商品"],
  ["categoryName", "商品分类"],
  ["parentCategoryName", "上级分类"],
  ["brandName", "品牌"],
  ["unitOfMeasureName", "计量单位"],
  ["barcode", "条码"],
  ["precisionScale", "数量小数位"],
  ["factorToBase", "换算为基本单位的系数"],
];
export function LinkedMasterDetail({ reference }: { reference: string }) {
  const target = masterDetailRoute(reference);
  const [item, setItem] = React.useState<
    (CoreMasterRecord | ProductMasterRecord) | null
  >(null);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setItem(null);
    setError(null);
    const selected = masterDetailRoute(reference);
    if (!selected) return;
    void request<{ item: CoreMasterRecord | ProductMasterRecord }>(
      `/api/v1/${selected.family}-master-data/${selected.kind}/${selected.id}`,
    )
      .then((result) => {
        if (
          result.item.id !== selected.id ||
          result.item.resourceType !== selected.kind
        )
          throw new Error("资料详情与链接不一致");
        if (active) setItem(result.item);
      })
      .catch((e) => {
        if (active) setError(toApiFailure(e));
      });
    return () => {
      active = false;
    };
  }, [reference, revision]);
  if (!target)
    return (
      <section className="workflow-page">
        <h1>基础资料链接无效</h1>
      </section>
    );
  const zeroCostAllowed =
    item && "allowZeroCost" in item ? item.allowZeroCost : null;
  const values = item as unknown as Record<string, unknown> | null;
  return (
    <section className="workflow-page" aria-label="基础资料详情">
      <h1>
        {target.label} · {item?.name ?? "详情"}
      </h1>
      {error ? (
        <PageLoadFailure
          failure={error}
          resourceLabel={target.label}
          onRetry={() => setRevision((v) => v + 1)}
        />
      ) : item ? (
        <>
          <p>
            {item.code} · {item.status === "active" ? "启用" : "停用"} · 版本{" "}
            {item.version}
          </p>
          <dl>
            {FIELDS.flatMap(([key, label]) =>
              values?.[key] == null
                ? []
                : [
                    <React.Fragment key={key}>
                      <dt>{label}</dt>
                      <dd>{String(values[key])}</dd>
                    </React.Fragment>,
                  ],
            )}
            {"creditLimitMinor" in item && item.creditLimitMinor !== null && (
              <>
                <dt>信用额度</dt>
                <dd>
                  {formatMoney(
                    item.creditCurrency ?? "",
                    item.creditLimitMinor / 100,
                  )}
                </dd>
              </>
            )}
            {zeroCostAllowed != null && (
              <>
                <dt>允许零成本</dt>
                <dd>{zeroCostAllowed ? "是" : "否"}</dd>
              </>
            )}
            {"usageScope" in item && item.usageScope && (
              <>
                <dt>换算用途</dt>
                <dd>
                  {
                    { sales: "销售", purchase: "采购", both: "采购和销售" }[
                      item.usageScope
                    ]
                  }
                </dd>
              </>
            )}
          </dl>
        </>
      ) : (
        <p role="status">正在加载资料…</p>
      )}
    </section>
  );
}
