import React from "react";
import {
  request,
  toApiFailure,
  type ApiFailure,
  type Envelope,
  type BusinessReturn,
} from "./api";
import { SearchIcon } from "./OrderWorkflowIcons";
import { formatAmount } from "./formatters";
import type { WorkflowModalState as ModalState } from "./OrderWorkflowModal";
export function WorkflowPage({
  domain,
  eyebrow,
  title,
  caption,
  primaryAction,
  secondaryAction,
  children,
}: React.PropsWithChildren<{
  domain: "sales" | "purchase";
  eyebrow: string;
  title: string;
  caption: string;
  primaryAction: React.ReactNode;
  secondaryAction: React.ReactNode;
}>) {
  return (
    <section className={`page order-workflow ${domain}`}>
      <div className="page-head workflow-head">
        <div>
          <p>{eyebrow}</p>
          <h1>{title}</h1>
          <span>{caption}</span>
        </div>
        <div className="workflow-head-actions">
          {secondaryAction}
          {primaryAction}
        </div>
      </div>
      {children}
    </section>
  );
}

export function WorkflowRail({
  active,
  stages,
  metrics,
  onSelect,
}: {
  active: string;
  stages: Array<{ id: string; code: string; label: string }>;
  metrics: string[];
  onSelect: (id: string) => void;
}) {
  return (
    <nav className="workflow-rail" aria-label="订单闭环阶段">
      {stages.map((stage, index) => (
        <React.Fragment key={stage.id}>
          <button
            type="button"
            className={active === stage.id ? "active" : ""}
            aria-current={active === stage.id ? "step" : undefined}
            onClick={() => onSelect(stage.id)}
          >
            <span>{stage.code}</span>
            <strong>{stage.label}</strong>
            <small>{metrics[index]}</small>
          </button>
          {index < stages.length - 1 && <i aria-hidden="true" />}
        </React.Fragment>
      ))}
    </nav>
  );
}

export function WorkflowPulse({
  items,
}: {
  items: Array<{ label: string; value: string; note: string }>;
}) {
  return (
    <div className="workflow-pulse">
      {items.map((item) => (
        <div key={item.label}>
          <span>{item.label}</span>
          <strong>{item.value}</strong>
          <small>{item.note}</small>
        </div>
      ))}
      <div className="workflow-rule-note">
        <span>闭环规则</span>
        <strong>先确认，再形成业务事实</strong>
        <small>草稿不会改变库存、应收或应付</small>
      </div>
    </div>
  );
}

export function WorkflowToolbar({
  query,
  onQuery,
  placeholder,
  filters,
  meta,
}: {
  query: string;
  onQuery: (value: string) => void;
  placeholder: string;
  filters?: React.ReactNode;
  meta: string;
}) {
  return (
    <div className="workflow-toolbar">
      <label>
        <SearchIcon />
        <span className="sr-only">搜索业务单据</span>
        <input
          type="search"
          value={query}
          placeholder={placeholder}
          onChange={(event) => onQuery(event.target.value)}
        />
      </label>
      {filters}
      <small>
        <i /> {meta}
      </small>
    </div>
  );
}

type DimensionOption = { id: string; label: string };

export function WorkflowDimensionFilters({
  legalEntityId,
  legalEntityOptions,
  onLegalEntity,
  businessUnitId,
  businessUnitOptions,
  onBusinessUnit,
}: {
  legalEntityId: string;
  legalEntityOptions: DimensionOption[];
  onLegalEntity: (value: string) => void;
  businessUnitId: string;
  businessUnitOptions: DimensionOption[];
  onBusinessUnit: (value: string) => void;
}) {
  return (
    <div className="workflow-toolbar-filters">
      <select
        aria-label="筛选法定主体"
        value={legalEntityId}
        onChange={(event) => onLegalEntity(event.target.value)}
      >
        <option value="">全部法定主体</option>
        {legalEntityOptions.map((option) => (
          <option value={option.id} key={option.id}>
            {option.label}
          </option>
        ))}
      </select>
      <select
        aria-label="筛选经营主体"
        value={businessUnitId}
        onChange={(event) => onBusinessUnit(event.target.value)}
      >
        <option value="">全部经营主体</option>
        {businessUnitOptions.map((option) => (
          <option value={option.id} key={option.id}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}

export function dimensionOptions<T>(
  rows: T[],
  idOf: (row: T) => string | undefined,
  labelOf: (row: T) => string | undefined,
) {
  const options = new Map<string, string>();
  for (const row of rows) {
    const id = idOf(row);
    if (id) options.set(id, labelOf(row) ?? id);
  }
  return [...options].map(([id, label]) => ({ id, label }));
}

export function dimensionLabel(
  name: string | undefined,
  code: string | undefined,
) {
  if (name && code) return `${name} · ${code}`;
  return name ?? code;
}

export async function loadWorkflowStage<T>(path: string): Promise<{
  items: T[];
  error: ApiFailure | null;
}> {
  try {
    const response = await request<Envelope<T>>(path);
    return { items: response.items, error: null };
  } catch (reason) {
    return {
      items: [],
      error: toApiFailure(reason, "业务数据加载失败，请重试"),
    };
  }
}

export function compactErrors<T extends string>(
  errors: Record<T, ApiFailure | null>,
): Partial<Record<T, ApiFailure>> {
  return Object.fromEntries(
    Object.entries(errors).filter((entry): entry is [string, ApiFailure] =>
      Boolean(entry[1]),
    ),
  ) as Partial<Record<T, ApiFailure>>;
}

export function workflowMetric(error: ApiFailure | undefined, value: string) {
  return error ? "暂不可用" : value;
}

export function workflowValue(error: ApiFailure | undefined, value: string) {
  return error ? "—" : value;
}

export function workflowNote(error: ApiFailure | undefined, value: string) {
  return error ? "当前账号无权读取" : value;
}

export function filterRows<T>(
  rows: T[],
  search: string,
  terms: (row: T) => Array<string | null | undefined>,
) {
  if (!search) return rows;
  return rows.filter((row) =>
    terms(row).some((term) => term?.toLowerCase().includes(search)),
  );
}

export function sum<T>(rows: T[] | undefined, key: keyof T) {
  return (rows ?? []).reduce((total, item) => total + Number(item[key]), 0);
}

export function money(value: number) {
  return `¥ ${formatAmount(value)}`;
}

export function ratio(value: number, total: number) {
  return total === 0 ? "—" : `${Math.round((value / total) * 100)}%`;
}

export function returnConfirmation(
  item: BusinessReturn,
  side: "sales" | "purchase",
): Extract<ModalState, { kind: "command" }> {
  const sales = side === "sales";
  return {
    kind: "command",
    title: `确认${sales ? "销售" : "采购"}退货`,
    description: sales
      ? "确认后商品按原出库冻结成本入库，并冲减对应未结经营应收与订单利润事实。"
      : "确认后商品按当前移动平均成本出库，并按原收货价税金额冲减对应未结经营应付。",
    path: `/api/v1/${sales ? "sales-returns" : "purchase-returns"}/${item.id}/confirm`,
    body: { expectedVersion: item.version },
    confirmLabel: "确认退货并写入业务事实",
  };
}
