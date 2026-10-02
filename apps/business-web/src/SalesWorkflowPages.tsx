import React from "react";
import {
  type ApiFailure,
  type BusinessReturn,
  type Receipt,
  type Receivable,
  type SalesOrder,
  type Shipment,
  toApiFailure,
} from "./api";
import { FulfillmentQueue } from "./FulfillmentQueue";
import { LinkedOrderDetail } from "./LinkedOrderDetail";
import "./order-workflow-detail.css";
import "./order-workflows.css";
import { PlusIcon, TruckIcon } from "./OrderWorkflowIcons";
import {
  WorkflowDimensionFilters,
  WorkflowPage,
  WorkflowRail,
  WorkflowToolbar,
  compactErrors,
  dimensionLabel,
  dimensionOptions,
  filterRows,
  loadWorkflowStage,
  returnConfirmation,
  workflowMetric,
} from "./OrderWorkflowLayout";
import {
  RecordDetail,
  WorkflowModal,
  type WorkflowModalState,
} from "./OrderWorkflowModal";
import {
  ReceiptsRegister,
  ReceivablesRegister,
  ReturnsRegister,
  SalesOrdersRegister,
  ShipmentsRegister,
} from "./OrderWorkflowRegisters";
import {
  CommandConfirmation,
  WorkflowError,
  useWorkflowData,
} from "./OrderWorkflowSupport";
import {
  ReturnAnalyticsPanel,
  SalesReturnInspection,
} from "./ReturnDispositionForms";
import { SalesReturnEntry } from "./ReturnEntry";
import { SalesOrderConfirmation } from "./SalesOrderConfirmation";
import { SalesOrderEntry } from "./SalesOrderEntry";
import { ServiceProjects } from "./ServiceProjects";
import {
  CustomerReceiptEntry,
  CustomerReceiptSettlement,
} from "./SettlementForms";
import { ShipmentConfirmation } from "./ShipmentConfirmation";
import { ShipmentEntry } from "./ShipmentEntry";
type SalesTab =
  | "orders"
  | "fulfillment"
  | "receivables"
  | "settlement"
  | "returns"
  | "projects"
  | "tasks";
type ModalState = WorkflowModalState;
type Mode = "orders" | "goods" | "service";
type SalesWorkflowData = {
  orders: SalesOrder[];
  shipments: Shipment[];
  receivables: Receivable[];
  receipts: Receipt[];
  returns: BusinessReturn[];
  errors: Partial<Record<SalesTab, ApiFailure>>;
};
const stagesFor = (
  mode: Mode,
): Array<{ id: SalesTab; code: string; label: string }> =>
  (mode === "orders"
    ? []
    : mode === "goods"
      ? [
          ["fulfillment", "出库履约"],
          ["receivables", "商品应收"],
          ["settlement", "收款核销"],
          ["returns", "销售退货"],
        ]
      : [
          ["projects", "服务项目"],
          ["tasks", "交付事项"],
          ["receivables", "服务应收"],
          ["settlement", "收款核销"],
        ]
  ).map(([id, label], i) => ({
    id: id as SalesTab,
    label,
    code: String(i + 1).padStart(2, "0"),
  }));
function readTab(mode: Mode): SalesTab {
  if (mode === "orders") return "orders";
  const [section, query = ""] = location.hash.slice(1).split("?");
  if (section === "serviceDeliverables") return "tasks";
  const legacy: Record<string, SalesTab> = {
    shipments: "fulfillment",
    receivables: "receivables",
    receipts: "settlement",
    serviceProjects: "projects",
  };
  const candidate = legacy[section] || new URLSearchParams(query).get("tab");
  return (
    stagesFor(mode).find((s) => s.id === candidate)?.id ||
    (mode === "goods" ? "fulfillment" : "projects")
  );
}
export function SalesOrderWorkflowPage({
  id,
  mode = "orders",
}: {
  id?: string;
  mode?: Mode;
}) {
  return id ? (
    <LinkedOrderDetail key={id} domain="sales" id={id} />
  ) : (
    <SalesOrderRegisterPage key={mode} mode={mode} />
  );
}

function SalesOrderRegisterPage({ mode }: { mode: Mode }) {
  const salesStages = stagesFor(mode);
  const [tab, setTab] = React.useState<SalesTab>(() => readTab(mode));
  const [revision, setRevision] = React.useState(0);
  const [modal, setModal] = React.useState<ModalState | null>(null);
  const [query, setQuery] = React.useState("");
  const [legalEntityId, setLegalEntityId] = React.useState("");
  const [businessUnitId, setBusinessUnitId] = React.useState("");
  const state = useWorkflowData<SalesWorkflowData>(async () => {
    const [orders, shipments, receivables, receipts, returns] =
      await Promise.all([
        mode === "orders"
          ? loadWorkflowStage<SalesOrder>("/api/v1/sales-orders?limit=200")
          : Promise.resolve({ items: [] as SalesOrder[], error: null }),
        mode === "goods"
          ? loadWorkflowStage<Shipment>("/api/v1/shipments?limit=200")
          : Promise.resolve({ items: [] as Shipment[], error: null }),
        mode !== "orders"
          ? loadWorkflowStage<Receivable>(
              "/api/v1/trade-receivables?limit=200&sourceKind=" +
                (mode === "service" ? "service" : "goods"),
            )
          : Promise.resolve({ items: [] as Receivable[], error: null }),
        mode !== "orders"
          ? loadWorkflowStage<Receipt>("/api/v1/customer-receipts?limit=200")
          : Promise.resolve({ items: [] as Receipt[], error: null }),
        mode === "goods"
          ? loadWorkflowStage<BusinessReturn>("/api/v1/sales-returns?limit=200")
          : Promise.resolve({ items: [] as BusinessReturn[], error: null }),
      ]);
    return {
      orders: orders.items,
      shipments: shipments.items,
      receivables: receivables.items.filter((r) =>
        mode === "service" ? !!r.serviceProjectId : !!r.shipmentId,
      ),
      receipts: receipts.items,
      returns: returns.items,
      errors: compactErrors<SalesTab>({
        projects: null,
        tasks: null,
        orders: orders.error,
        fulfillment: shipments.error,
        receivables: receivables.error,
        settlement: receipts.error,
        returns: returns.error,
      }),
    };
  }, [revision]);
  React.useEffect(() => {
    const sync = () => setTab(readTab(mode));
    window.addEventListener("hashchange", sync);
    return () => window.removeEventListener("hashchange", sync);
  }, [mode]);
  const selectTab = (value: string) => {
    setTab(value as SalesTab);
    setQuery("");
    const section = mode === "goods" ? "goodsOrders" : "serviceOrders";
    location.hash = `${section}?tab=${value}`;
  };
  const data = state.data;
  const stageError = data?.errors[tab] ?? null;
  const search = query.trim().toLowerCase();
  const sourceOrders = data?.orders ?? [];
  const legalEntityOptions = dimensionOptions(
    sourceOrders,
    (item) => item.legalEntityId,
    (item) => dimensionLabel(item.legalEntityName, item.legalEntityCode),
  );
  const businessUnitOptions = dimensionOptions(
    sourceOrders,
    (item) => item.businessUnitId,
    (item) => dimensionLabel(item.businessUnitName, item.businessUnitCode),
  );
  const orders = filterRows(sourceOrders, search, (item) => [
    item.orderNumber,
    item.customerId,
    item.customerCode,
    item.customerName,
    item.legalEntityCode,
    item.legalEntityName,
    item.businessUnitCode,
    item.businessUnitName,
    item.lifecycleStatus,
  ]).filter(
    (item) =>
      (!legalEntityId || item.legalEntityId === legalEntityId) &&
      (!businessUnitId || item.businessUnitId === businessUnitId),
  );
  const shipments = filterRows(data?.shipments ?? [], search, (item) => [
    item.shipmentNumber,
    item.salesOrderId,
    item.status,
  ]);
  const receivables = filterRows(data?.receivables ?? [], search, (item) => [
    item.receivableNumber,
    item.customerId,
    item.salesOrderId,
    item.status,
  ]);
  const receipts = filterRows(data?.receipts ?? [], search, (item) => [
    item.receiptNumber,
    item.customerId,
    item.status,
  ]);
  const returns = filterRows(data?.returns ?? [], search, (item) => [
    item.returnNumber,
    item.sourceId,
    item.partnerId,
    item.reasonCode,
    item.status,
  ]);
  const refresh = () => setRevision((value) => value + 1);
  const done = () => {
    setModal(null);
    refresh();
  };

  return (
    <WorkflowPage
      domain="sales"
      eyebrow="业务闭环"
      title={
        mode === "orders"
          ? "销售订单"
          : mode === "goods"
            ? "商品订单闭环"
            : "服务订单闭环"
      }
      caption={
        mode === "orders"
          ? "统一创建商品、服务及混合销售订单，确认后分别履约。"
          : mode === "goods"
            ? "已确认订单中的商品行，经出库形成应收，再登记收款与核销。"
            : "已确认订单中的服务行，经项目交付和验收自动记账，再登记收款与核销。"
      }
      primaryAction={
        mode === "orders" && data && !data.errors.orders ? (
          <button
            type="button"
            onClick={() => setModal({ kind: "sales-create" })}
          >
            <PlusIcon /> 新增销售订单
          </button>
        ) : undefined
      }
      secondaryAction={
        mode === "goods" &&
        tab === "fulfillment" &&
        data &&
        !data.errors.fulfillment ? (
          <button
            type="button"
            className="secondary"
            onClick={() => setModal({ kind: "shipment-create" })}
          >
            <TruckIcon /> 新建出库单
          </button>
        ) : undefined
      }
    >
      {mode !== "orders" && (
        <WorkflowRail
          active={tab}
          stages={salesStages}
          onSelect={selectTab}
          metrics={salesStages.map((s) =>
            s.id === "projects"
              ? "交付与验收"
              : s.id === "tasks"
                ? "交付进度"
                : s.id === "settlement"
                  ? "共用收款台账"
                  : workflowMetric(
                      data?.errors[s.id],
                      `${s.id === "fulfillment" ? (data?.shipments.length ?? 0) : s.id === "returns" ? (data?.returns.length ?? 0) : (data?.receivables.length ?? 0)} 条`,
                    ),
          )}
        />
      )}
      {mode === "orders" && (
        <p>
          订单确认后，商品行进入商品订单闭环，服务行进入服务订单闭环；混合订单共用订单编号。
        </p>
      )}
      {tab === "settlement" && (
        <p>
          收款单共用。本页核销仅列出{mode === "service" ? "服务" : "商品"}
          应收；同一笔收款的未核销余额在两个闭环中共享。
        </p>
      )}
      {(tab === "projects" || tab === "fulfillment") && (
        <FulfillmentQueue service={mode === "service"} revision={revision} />
      )}
      {tab === "projects" || tab === "tasks" ? (
        <ServiceProjects
          key={tab}
          tasks={tab === "tasks"}
          embedded
          onChanged={refresh}
        />
      ) : (
        <>
          <WorkflowToolbar
            query={query}
            onQuery={setQuery}
            placeholder="搜索订单号、客户、出库单或应收单…"
            filters={
              tab === "orders" ? (
                <WorkflowDimensionFilters
                  legalEntityId={legalEntityId}
                  legalEntityOptions={legalEntityOptions}
                  onLegalEntity={setLegalEntityId}
                  businessUnitId={businessUnitId}
                  businessUnitOptions={businessUnitOptions}
                  onBusinessUnit={setBusinessUnitId}
                />
              ) : undefined
            }
            meta={
              state.loading
                ? "正在同步业务事实…"
                : `数据已同步 · v${revision + 1}`
            }
          />
          {state.error || stageError ? (
            <WorkflowError
              error={
                state.error ??
                stageError ??
                toApiFailure(null, "业务数据加载失败")
              }
              resourceLabel={
                tab === "orders"
                  ? "销售订单"
                  : salesStages.find((stage) => stage.id === tab)?.label ??
                    "销售闭环"
              }
              onRetry={refresh}
            />
          ) : (
            <div className="workflow-register" aria-busy={state.loading}>
              {tab === "orders" && (
                <SalesOrdersRegister rows={orders} onModal={setModal} />
              )}
              {tab === "fulfillment" && (
                <ShipmentsRegister rows={shipments} onModal={setModal} />
              )}
              {tab === "receivables" && (
                <ReceivablesRegister rows={receivables} onModal={setModal} />
              )}
              {tab === "settlement" && (
                <ReceiptsRegister
                  rows={receipts}
                  onModal={setModal}
                  onCreate={() => setModal({ kind: "customer-receipt-create" })}
                />
              )}
              {tab === "returns" && (
                <>
                  <ReturnAnalyticsPanel side="sales" />
                  <ReturnsRegister
                    rows={returns}
                    side="sales"
                    onModal={setModal}
                    onCreate={() => setModal({ kind: "sales-return-create" })}
                  />
                </>
              )}
            </div>
          )}
        </>
      )}
      {modal && (
        <WorkflowModal state={modal} onClose={() => setModal(null)}>
          {modal.kind === "sales-create" && <SalesOrderEntry onDone={done} />}
          {modal.kind === "sales-edit" && (
            <SalesOrderEntry orderId={modal.id} onDone={done} />
          )}
          {modal.kind === "sales-confirm" && (
            <SalesOrderConfirmation orderId={modal.id} onDone={done} />
          )}
          {modal.kind === "shipment-create" && <ShipmentEntry onDone={done} />}
          {modal.kind === "shipment-confirm" && (
            <ShipmentConfirmation shipmentId={modal.id} onDone={done} />
          )}
          {modal.kind === "customer-receipt-create" && (
            <CustomerReceiptEntry onDone={done} />
          )}
          {modal.kind === "customer-receipt-settle" && (
            <CustomerReceiptSettlement
              receipt={modal.receipt}
              receivables={data?.receivables ?? []}
              onDone={done}
            />
          )}
          {modal.kind === "sales-return-create" && (
            <SalesReturnEntry onDone={done} />
          )}
          {modal.kind === "sales-return-confirm" && (
            <CommandConfirmation
              state={returnConfirmation(modal.item, "sales")}
              onCancel={() => setModal(null)}
              onDone={done}
            />
          )}
          {modal.kind === "sales-return-inspect" && (
            <SalesReturnInspection item={modal.item} onDone={done} />
          )}
          {modal.kind === "record-detail" && (
            <RecordDetail state={modal} onEdit={setModal} />
          )}
          {modal.kind === "command" && (
            <CommandConfirmation
              state={modal}
              onCancel={() => setModal(null)}
              onDone={done}
            />
          )}
        </WorkflowModal>
      )}
    </WorkflowPage>
  );
}
