import { formatMoney } from "./formatters";
import React from "react";
import {
  type ApiFailure,
  type BusinessReturn,
  type GoodsReceipt,
  type Payable,
  type PurchaseOrder,
  type SupplierPayment,
  toApiFailure,
} from "./api";
import { GoodsReceiptConfirmation } from "./GoodsReceiptConfirmation";
import { GoodsReceiptEntry } from "./GoodsReceiptEntry";
import { LinkedOrderDetail } from "./LinkedOrderDetail";
import "./order-workflow-detail.css";
import "./order-workflows.css";
import { PlusIcon, ReceiveIcon } from "./OrderWorkflowIcons";
import {
  WorkflowDimensionFilters,
  WorkflowPage,
  WorkflowPulse,
  WorkflowRail,
  WorkflowToolbar,
  compactErrors,
  dimensionLabel,
  dimensionOptions,
  filterRows,
  loadWorkflowStage,
  ratio,
  returnConfirmation,
  workflowMetric,
  workflowNote,
  workflowValue,
} from "./OrderWorkflowLayout";
import {
  RecordDetail,
  WorkflowModal,
  type WorkflowModalState,
} from "./OrderWorkflowModal";
import {
  GoodsReceiptsRegister,
  PayablesRegister,
  PaymentsRegister,
  PurchaseOrdersRegister,
  ReturnsRegister,
} from "./OrderWorkflowRegisters";
import {
  CommandConfirmation,
  WorkflowError,
  useWorkflowData,
} from "./OrderWorkflowSupport";
import { PurchaseDeliveryPanel } from "./PurchaseDeliveryPanel";
import { PurchaseOrderConfirmation } from "./PurchaseOrderConfirmation";
import { PurchaseOrderEntry } from "./PurchaseOrderEntry";
import {
  PurchaseReturnAcknowledgment,
  PurchaseReturnDispatch,
  ReturnAnalyticsPanel,
} from "./ReturnDispositionForms";
import { PurchaseReturnEntry } from "./ReturnEntry";
import {
  SupplierPaymentEntry,
  SupplierPaymentSettlement,
} from "./SettlementForms";

type PurchaseTab =
  | "orders"
  | "delivery"
  | "receiving"
  | "payables"
  | "settlement"
  | "returns";
type ModalState = WorkflowModalState;

type PurchaseWorkflowData = {
  orders: PurchaseOrder[];
  receipts: GoodsReceipt[];
  payables: Payable[];
  payments: SupplierPayment[];
  returns: BusinessReturn[];
  errors: Partial<Record<PurchaseTab, ApiFailure>>;
};

const purchaseStages: Array<{ id: PurchaseTab; code: string; label: string }> =
  [
    { id: "orders", code: "01", label: "采购订单" },
    { id: "delivery", code: "02", label: "交期履约" },
    { id: "receiving", code: "03", label: "到货入库" },
    { id: "payables", code: "04", label: "经营应付" },
    { id: "settlement", code: "05", label: "付款核销" },
    { id: "returns", code: "06", label: "采购退货" },
  ];

export function PurchaseOrderWorkflowPage({ id }: { id?: string }) {
  return id ? (
    <LinkedOrderDetail key={id} domain="purchase" id={id} />
  ) : (
    <PurchaseOrderRegisterPage />
  );
}

function PurchaseOrderRegisterPage() {
  const [tab, setTab] = React.useState<PurchaseTab>("orders");
  const [revision, setRevision] = React.useState(0);
  const [modal, setModal] = React.useState<ModalState | null>(null);
  const [query, setQuery] = React.useState("");
  const [legalEntityId, setLegalEntityId] = React.useState("");
  const [businessUnitId, setBusinessUnitId] = React.useState("");
  const state = useWorkflowData<PurchaseWorkflowData>(async () => {
    const [orders, receipts, payables, payments, returns] = await Promise.all([
      loadWorkflowStage<PurchaseOrder>("/api/v1/purchase-orders?limit=200"),
      loadWorkflowStage<GoodsReceipt>("/api/v1/goods-receipts?limit=200"),
      loadWorkflowStage<Payable>("/api/v1/trade-payables?limit=200"),
      loadWorkflowStage<SupplierPayment>("/api/v1/supplier-payments?limit=200"),
      loadWorkflowStage<BusinessReturn>("/api/v1/purchase-returns?limit=200"),
    ]);
    return {
      orders: orders.items,
      receipts: receipts.items,
      payables: payables.items,
      payments: payments.items,
      returns: returns.items,
      errors: compactErrors<PurchaseTab>({
        orders: orders.error,
        delivery: null,
        receiving: receipts.error,
        payables: payables.error,
        settlement: payments.error,
        returns: returns.error,
      }),
    };
  }, [revision]);
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
    item.purchaseOrderNumber,
    item.supplierId,
    item.supplierCode,
    item.supplierName,
    item.legalEntityCode,
    item.legalEntityName,
    item.businessUnitCode,
    item.businessUnitName,
    item.lifecycleStatus,
    item.receivingStatus,
  ]).filter(
    (item) =>
      (!legalEntityId || item.legalEntityId === legalEntityId) &&
      (!businessUnitId || item.businessUnitId === businessUnitId),
  );
  const receipts = filterRows(data?.receipts ?? [], search, (item) => [
    item.goodsReceiptNumber,
    item.purchaseOrderId,
    item.supplierId,
    item.status,
  ]);
  const payables = filterRows(data?.payables ?? [], search, (item) => [
    item.payableNumber,
    item.purchaseOrderId,
    item.supplierId,
    item.status,
  ]);
  const payments = filterRows(data?.payments ?? [], search, (item) => [
    item.supplierPaymentNumber,
    item.supplierId,
    item.status,
  ]);
  const returns = filterRows(data?.returns ?? [], search, (item) => [
    item.returnNumber,
    item.sourceId,
    item.partnerId,
    item.reasonCode,
    item.status,
  ]);
  const activePayables = payables.filter(item => item.status !== "reversed");
  const openPayable = currencyTotals(activePayables, item => item.openAmount);
  const receivedOrders = orders.filter(item => item.receivingStatus === "fully_received").length;
  const refresh = () => setRevision((value) => value + 1);
  const done = () => {
    setModal(null);
    refresh();
  };

  return (
    <WorkflowPage
      domain="purchase"
      eyebrow="采购闭环 / Procure to pay"
      title="采购订单闭环"
      caption="从采购承诺、实际到货、移动平均成本到经营应付与付款核销，所有变化保留来源凭据。"
      primaryAction={
        data && !data.errors.orders ? (
          <button
            type="button"
            onClick={() => setModal({ kind: "purchase-create" })}
          >
            <PlusIcon /> 新增采购订单
          </button>
        ) : undefined
      }
      secondaryAction={
        data && !data.errors.receiving ? (
          <button
            type="button"
            className="secondary"
            onClick={() => setModal({ kind: "receipt-create" })}
          >
            <ReceiveIcon /> 新建收货单
          </button>
        ) : undefined
      }
    >
      <WorkflowRail
        active={tab}
        stages={purchaseStages}
        onSelect={(value) => setTab(value as PurchaseTab)}
        metrics={[
          workflowMetric(data?.errors.orders, `${data?.orders.length ?? 0} 单`),
          "交期跟踪",
          workflowMetric(
            data?.errors.receiving,
            `${data?.receipts.length ?? 0} 次`,
          ),
          workflowMetric(data?.errors.payables, `待付 ${openPayable}`),
          workflowMetric(
            data?.errors.settlement,
            `${data?.payments.length ?? 0} 笔`,
          ),
          workflowMetric(
            data?.errors.returns,
            `${data?.returns.length ?? 0} 笔`,
          ),
        ]}
      />
      <WorkflowPulse
        items={[
          {
            label: "已加载采购金额",
            value: workflowValue(
              data?.errors.orders,
              currencyTotals(orders, item => item.grossAmount),
            ),
            note: workflowNote(
              data?.errors.orders,
              `筛选后 ${orders.length} 张订单，含草稿`,
            ),
          },
          {
            label: "已到齐订单",
            value: workflowValue(data?.errors.orders, String(receivedOrders)),
            note: workflowNote(
              data?.errors.orders,
              `已加载订单到货率 ${ratio(receivedOrders, orders.length)}`,
            ),
          },
          {
            label: "已加载应付余额",
            value: workflowValue(data?.errors.payables, openPayable),
            note: workflowNote(
              data?.errors.payables,
              `${activePayables.filter(item => Number(item.openAmount) > 0).length} 笔未结，按币种分别统计`,
            ),
          },
        ]}
      />
      <WorkflowToolbar
        query={query}
        onQuery={setQuery}
        placeholder="搜索采购单、供应商、收货单或应付单…"
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
          state.loading ? "正在同步业务事实…" : `数据已同步 · v${revision + 1}`
        }
      />
      {state.error || stageError ? (
        <WorkflowError
          error={
            state.error ?? stageError ?? toApiFailure(null, "业务数据加载失败")
          }
          resourceLabel={
            purchaseStages.find((stage) => stage.id === tab)?.label ??
            "采购闭环"
          }
          onRetry={refresh}
        />
      ) : (
        <div className="workflow-register" aria-busy={state.loading}>
          {tab === "orders" && (
            <PurchaseOrdersRegister rows={orders} onModal={setModal} />
          )}
          {tab === "delivery" && <PurchaseDeliveryPanel onChanged={refresh} />}
          {tab === "receiving" && (
            <GoodsReceiptsRegister rows={receipts} onModal={setModal} />
          )}
          {tab === "payables" && (
            <PayablesRegister rows={payables} onModal={setModal} />
          )}
          {tab === "settlement" && (
            <PaymentsRegister
              rows={payments}
              onModal={setModal}
              onCreate={() => setModal({ kind: "supplier-payment-create" })}
            />
          )}
          {tab === "returns" && (
            <>
              <ReturnAnalyticsPanel side="purchase" />
              <ReturnsRegister
                rows={returns}
                side="purchase"
                onModal={setModal}
                onCreate={() => setModal({ kind: "purchase-return-create" })}
              />
            </>
          )}
        </div>
      )}
      {modal && (
        <WorkflowModal state={modal} onClose={() => setModal(null)}>
          {modal.kind === "purchase-create" && (
            <PurchaseOrderEntry onDone={done} />
          )}
          {modal.kind === "purchase-edit" && (
            <PurchaseOrderEntry orderId={modal.id} onDone={done} />
          )}
          {modal.kind === "purchase-confirm" && (
            <PurchaseOrderConfirmation orderId={modal.id} onDone={done} />
          )}
          {modal.kind === "receipt-create" && (
            <GoodsReceiptEntry onDone={done} />
          )}
          {modal.kind === "receipt-confirm" && (
            <GoodsReceiptConfirmation receiptId={modal.id} onDone={done} />
          )}
          {modal.kind === "supplier-payment-create" && (
            <SupplierPaymentEntry onDone={done} />
          )}
          {modal.kind === "supplier-payment-settle" && (
            <SupplierPaymentSettlement
              payment={modal.payment}
              payables={data?.payables ?? []}
              onDone={done}
            />
          )}
          {modal.kind === "purchase-return-create" && (
            <PurchaseReturnEntry onDone={done} />
          )}
          {modal.kind === "purchase-return-confirm" && (
            <CommandConfirmation
              state={returnConfirmation(modal.item, "purchase")}
              onCancel={() => setModal(null)}
              onDone={done}
            />
          )}
          {modal.kind === "purchase-return-dispatch" && (
            <PurchaseReturnDispatch item={modal.item} onDone={done} />
          )}
          {modal.kind === "purchase-return-acknowledge" && (
            <PurchaseReturnAcknowledgment item={modal.item} onDone={done} />
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

function currencyTotals<T extends { currency: string }>(rows: T[], amount: (row: T) => string) {
  const totals = new Map<string, number>();
  for (const row of rows) totals.set(row.currency, (totals.get(row.currency) ?? 0) + Number(amount(row)));
  return totals.size ? [...totals].sort(([a], [b]) => a.localeCompare(b)).map(([currency, total]) => formatMoney(currency, total)).join(" / ") : "暂无记录";
}
