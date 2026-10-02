import { useOrderDraft } from "./OrderDraft";
import { useOrderValidation } from "./OrderValidation";
import "./order-entry-responsive.css";
import { OrderMasterPicker } from "./OrderMasterPicker";
import React from "react";
import type { CrmDetail } from "./crm";
import { useCrmCommand } from "./useCrmCommand";
import type { SalesOrderDraftOptions } from "./salesOrderDraftOptions";
import {
  type CoreMasterRecord,
  type MasterDataList,
  type MasterDataRecord,
  request,
} from "./api";
import { formatAmount } from "./formatters";
import { loadOperatingUnits } from "./operatingUnitOptions";
import { OperatingUnitPicker } from "./OperatingUnitPicker";
import {
  rememberSyncedRecentOperatingUnit,
  resolveRecentOperatingUnit,
} from "./recentOperatingUnit";
import {
  isCompleteSalesOrderLine,
  newSalesOrderLine,
  type SalesOrderLineDraft,
} from "./salesOrderEntryDraft";

type Catalog = {
  legalEntities: MasterDataRecord[];
  customers: MasterDataRecord[];
  businessUnits: CoreMasterRecord[];
  skus: MasterDataRecord[];
  warehouses: MasterDataRecord[];
  units: MasterDataRecord[];
};

const emptyCatalog: Catalog = {
  legalEntities: [],
  customers: [],
  businessUnits: [],
  skus: [],
  warehouses: [],
  units: [],
};

export function SalesOrderEntry({
  onDone,
  orderId,
  opportunityId,
  onBusy,
}: {
  onDone: () => void;
  orderId?: string;
  opportunityId?: string;
  onBusy?: (busy: boolean) => void;
}) {
  const command = useCrmCommand();
  const draftGuard = useOrderDraft();
  const validation = useOrderValidation();
  const [source, setSource] = React.useState<CrmDetail["item"] | null>(null);
  const [original, setOriginal] = React.useState<
    SalesOrderDraftOptions["draft"] | null
  >(null);
  const [ready, setReady] = React.useState(false);
  const [catalog, setCatalog] = React.useState<Catalog>(emptyCatalog);
  const [loading, setLoading] = React.useState(true);
  const [legalEntityId, setLegalEntityId] = React.useState("");
  const [customerId, setCustomerId] = React.useState("");
  const [businessUnitId, setBusinessUnitId] = React.useState("");
  const [orderDate, setOrderDate] = React.useState(today());
  const [requestedDeliveryDate, setRequestedDeliveryDate] = React.useState("");
  const [customerReference, setCustomerReference] = React.useState("");
  const [businessNote, setBusinessNote] = React.useState("");
  const [lines, setLines] = React.useState<SalesOrderLineDraft[]>([
    newSalesOrderLine(),
  ]);
  const [busy, setBusy] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);

  React.useEffect(() => {
    let active = true;
    setLoading(true);
    setReady(false);
    Promise.all([
      loadMaster("legal_entity"),
      loadMaster("customer"),
      loadOperatingUnits(),
      loadMaster("sku"),
      loadMaster("warehouse"),
      loadMaster("unit_of_measure"),
      orderId
        ? request<SalesOrderDraftOptions>(
            `/api/v1/sales-orders/${orderId}/draft-options`,
          )
        : Promise.resolve(null),
      opportunityId
        ? request<CrmDetail>(
            `/api/v1/crm/opportunities/${encodeURIComponent(opportunityId)}`,
          )
        : Promise.resolve(null),
    ])
      .then(
        ([
          legalEntities,
          customers,
          businessUnits,
          skus,
          warehouses,
          units,
          options,
          opportunity,
        ]) => {
          if (!active) return;
          const next = {
            legalEntities,
            customers,
            businessUnits,
            skus,
            warehouses,
            units,
          };
          setCatalog(next);
          if (options) {
            if (
              !options.canUpdate ||
              options.draft.lifecycleStatus !== "draft"
            ) {
              throw new Error("此订单当前不可编辑，请检查权限或刷新订单状态。");
            }
            const draft = options.draft;
            setOriginal(draft);
            setLegalEntityId(draft.legalEntityId);
            setCustomerId(draft.customerId);
            setBusinessUnitId(draft.businessUnitId);
            setOrderDate(draft.orderDate);
            setRequestedDeliveryDate(draft.requestedDeliveryDate ?? "");
            setCustomerReference(draft.customerReference ?? "");
            setBusinessNote(draft.businessNote ?? "");
            setLines(
              draft.lines.map((line) => ({
                ...line,
                key: crypto.randomUUID(),
                warehouseId: line.warehouseId ?? "",
                taxRate: String(Number(line.taxRate) * 100),
              })),
            );
            setReady(true);
            return;
          }
          if (opportunity) {
            const source = opportunity.item;
            if (source.stage !== "won" || !source.customerId)
              throw new Error("商机须已成交并关联正式客户后才能创建订单草稿。");
            if (
              !legalEntities.some((v) => v.id === source.legalEntityId) ||
              !customers.some((v) => v.id === source.customerId) ||
              !businessUnits.some((v) => v.id === source.businessUnitId)
            )
              throw new Error(
                "商机对应的客户或主体不可用于录单，请核对权限及启用状态。",
              );
            setSource(source);
            setLegalEntityId(source.legalEntityId);
            setCustomerId(source.customerId);
            setBusinessUnitId(source.businessUnitId);
            setCustomerReference(`CRM:${source.id}`);
            setBusinessNote(
              `来自已成交商机：${source.title}\n联系人：${source.contactName} ${source.contactDetails}\n商机：/#crm?opportunity=${encodeURIComponent(source.id)}`,
            );
            setLines([newSalesOrderLine()]);
            setReady(true);
            return;
          }
          setReady(true);
          setLegalEntityId(legalEntities[0]?.id ?? "");
          setCustomerId(customers[0]?.id ?? "");
          setBusinessUnitId(
            resolveRecentOperatingUnit(
              "sales-order",
              businessUnits,
              businessUnits[0]?.id ?? "",
            ),
          );
          setLines([
            newSalesOrderLine(skus[0]?.id, warehouses[0]?.id, units[0]?.id),
          ]);
        },
      )
      .catch((error: Error) => active && setNotice(error.message))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, [orderId, opportunityId]);

  const availableCustomers = catalog.customers.filter(
    (item) => !item.legalEntityId || item.legalEntityId === legalEntityId,
  );
  const availableUnits = catalog.businessUnits;
  const availableWarehouses = catalog.warehouses.filter(
    (item) => !item.legalEntityId || item.legalEntityId === legalEntityId,
  );

  function changeLegalEntity(value: string) {
    setLegalEntityId(value);
    const customers = catalog.customers.filter(
      (item) => !item.legalEntityId || item.legalEntityId === value,
    );
    const warehouses = catalog.warehouses.filter(
      (item) => !item.legalEntityId || item.legalEntityId === value,
    );
    setCustomerId((current) =>
      customers.some((item) => item.id === current)
        ? current
        : (customers[0]?.id ?? ""),
    );
    setLines((current) =>
      current.map((line) => ({
        ...line,
        warehouseId: warehouses.some((item) => item.id === line.warehouseId)
          ? line.warehouseId
          : (warehouses[0]?.id ?? ""),
      })),
    );
  }

  function updateLine(
    key: string,
    field: keyof SalesOrderLineDraft,
    value: string,
  ) {
    setLines((current) =>
      current.map((line) =>
        line.key === key ? { ...line, [field]: value } : line,
      ),
    );
  }

  const isService = (sku: string) => {
    const kind = catalog.skus.find((s) => s.id === sku)?.serviceKind;
    return kind === "technical_service" || kind === "software_service";
  };
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!validation.validate(event.currentTarget as HTMLFormElement)) return;
    setNotice(null);
    if (!ready || busy) return;
    if (!legalEntityId || !customerId || !businessUnitId) {
      setNotice("请选择法律主体、客户和业务单元。");
      return;
    }
    if (
      lines.some(
        (line) => !isCompleteSalesOrderLine(line, isService(line.skuId)),
      )
    ) {
      setNotice("请补全商品行，并填写单价；数量须大于 0，单价不能为负。");
      return;
    }
    draftGuard.setBusy(true);
    setBusy(true);
    onBusy?.(true);
    try {
      const send = opportunityId ? command : request;
      const output = await send<{ number: string }>(
        orderId ? `/api/v1/sales-orders/${orderId}` : "/api/v1/sales-orders",
        {
          method: orderId ? "PUT" : "POST",
          body: JSON.stringify({
            ...(orderId && original
              ? {
                  expectedVersion: original.version,
                  departmentId: original.departmentId,
                  brandId: original.brandId,
                  paymentTermsDays: original.paymentTermsDays,
                }
              : { legalEntityId }),
            customerId,
            businessUnitId,
            currency: original?.currency ?? source?.currency ?? "CNY",
            orderDate,
            requestedDeliveryDate: requestedDeliveryDate || undefined,
            customerReference: customerReference.trim() || undefined,
            businessNote: businessNote.trim() || undefined,
            lines: lines.map(({ key: _key, ...line }) => ({
              ...line,
              warehouseId: isService(line.skuId) ? null : line.warehouseId,
              taxRate: String(Number(line.taxRate) / 100),
            })),
          }),
        },
      );
      setNotice(`销售订单 ${output.number} 已保存为草稿。`);
      void rememberSyncedRecentOperatingUnit("sales-order", businessUnitId);
      setCustomerReference("");
      setBusinessNote("");
      setLines([
        newSalesOrderLine(
          catalog.skus[0]?.id,
          availableWarehouses[0]?.id,
          catalog.units[0]?.id,
        ),
      ]);
      draftGuard.saved();
      onDone();
    } catch (error) {
      setNotice((error as Error).message);
    } finally {
      draftGuard.setBusy(false);
      setBusy(false);
      onBusy?.(false);
    }
  }

  const totals = lines.reduce(
    (sum, line) => {
      const base = amount(line.quantity) * amount(line.unitPrice);
      const discount = amount(line.discountAmount);
      const net = Math.max(0, base - discount);
      const tax = net * (amount(line.taxRate) / 100);
      return {
        subtotal: sum.subtotal + base,
        discount: sum.discount + discount,
        tax: sum.tax + tax,
        gross: sum.gross + net + tax,
      };
    },
    { subtotal: 0, discount: 0, tax: 0, gross: 0 },
  );

  return (
    <section className="sales-entry" aria-labelledby="sales-entry-title">
      <header>
        <div>
          <span>{orderId ? "EDIT SALES ORDER" : "NEW SALES ORDER"}</span>
          <h2 id="sales-entry-title">
            {orderId ? "编辑销售订单草稿" : "录入销售订单"}
          </h2>
          <p>先保存草稿，再进入订单详情核对库存并执行确认。</p>
          {source && (
            <p>
              来源商机：{source.title} · {source.currency}
            </p>
          )}
        </div>
        <strong>草稿</strong>
      </header>
      {loading ? (
        <p className="entry-loading">正在加载可用客户、商品与仓库…</p>
      ) : !ready ? (
        <p className="entry-notice" role="alert">
          {notice}
        </p>
      ) : (
        <form onSubmit={submit} noValidate
          onChangeCapture={(event) => { if (!(event.target instanceof HTMLInputElement && event.target.type === "search")) draftGuard.markDirty(); }}
          onClickCapture={(event) => {
            const button = (event.target as HTMLElement).closest("button");
            if (button && (button.matches(".line-remove, .master-tree-choice") || button.textContent?.includes("添加商品行") || button.textContent?.includes("添加采购行"))) draftGuard.markDirty();
          }} onInput={validation.clear} onChange={validation.clear} onClick={(event) => { if ((event.target as HTMLElement).closest("[role=option]")) validation.clear(); }}>
          {validation.summary}
          <div className="entry-fields">
            <Field label="法律主体">
              <select
                value={legalEntityId}
                disabled={Boolean(orderId)}
                onChange={(event) => changeLegalEntity(event.target.value)}
                required
              >
                {catalog.legalEntities.map(option)}
              </select>
            </Field>
            <OrderMasterPicker label="客户" noun="客户" inLine={false}
              value={customerId} items={availableCustomers} onChange={setCustomerId} />
            <OperatingUnitPicker
              orderRequired
              label="经营主体"
              records={availableUnits}
              value={businessUnitId}
              onChange={setBusinessUnitId}
              preferenceContext={
                orderId || opportunityId ? undefined : "sales-order"
              }
              preferenceFallback={availableUnits[0]?.id ?? ""}
            />
            <Field label="订单日期">
              <input
                type="date"
                value={orderDate}
                onChange={(event) => setOrderDate(event.target.value)}
                required
              />
            </Field>
            <Field label="要求交付日">
              <input
                type="date"
                min={orderDate}
                value={requestedDeliveryDate}
                onChange={(event) =>
                  setRequestedDeliveryDate(event.target.value)
                }
              />
            </Field>
            <Field label="客户参考号">
              <input
                readOnly={
                  !!source ||
                  /^CRM:[0-9a-f-]{36}$/i.test(original?.customerReference ?? "")
                }
                value={customerReference}
                maxLength={120}
                onChange={(event) => setCustomerReference(event.target.value)}
                placeholder="可选"
              />
            </Field>
          </div>

          <div className="entry-lines">
            <div className="entry-line-head">
              <span>商品</span>
              <span>仓库</span>
              <span>单位</span>
              <span>数量</span>
              <span>单价</span>
              <span>折扣</span>
              <span>税率 %</span>
              <span />
            </div>
            {lines.map((line, index) => (
              <div className="entry-line" key={line.key}>
                <OrderMasterPicker label={`第 ${index + 1} 行商品`} value={line.skuId}
                  items={catalog.skus} onChange={(value) => updateLine(line.key, "skuId", value)} />
                <OrderMasterPicker label={`第 ${index + 1} 行仓库`} noun="仓库"
                  disabled={isService(line.skuId)} placeholder={isService(line.skuId) ? "服务无需仓库" : "请选择仓库"}
                  value={isService(line.skuId) ? "" : line.warehouseId} items={availableWarehouses}
                  onChange={(value) => updateLine(line.key, "warehouseId", value)} />
                <label>
                  <span>单位</span>
                  <select
                    aria-label={`第 ${index + 1} 行单位`}
                    value={line.unitOfMeasureId}
                    onChange={(event) =>
                      updateLine(
                        line.key,
                        "unitOfMeasureId",
                        event.target.value,
                      )
                    }
                    required
                  >
                    {source && <option value="">请选择单位</option>}
                    {catalog.units.map(option)}
                  </select>
                </label>
                {(
                  [
                    "quantity",
                    "unitPrice",
                    "discountAmount",
                    "taxRate",
                  ] as const
                ).map((field) => (
                  <label key={field}>
                    <span>{lineLabel(field)}</span>
                    <input
                      aria-label={`第 ${index + 1} 行${lineLabel(field)}`}
                      type="number"
                      min={field === "quantity" ? "0.000001" : "0"}
                      step="0.000001"
                      value={line[field]}
                      placeholder={field === "unitPrice" ? "必填" : undefined}
                      onChange={(event) =>
                        updateLine(line.key, field, event.target.value)
                      }
                      required
                    />
                  </label>
                ))}
                <button
                  type="button"
                  className="line-remove secondary"
                  onClick={() =>
                    setLines((current) =>
                      current.length === 1
                        ? current
                        : current.filter((item) => item.key !== line.key),
                    )
                  }
                  disabled={lines.length === 1}
                  aria-label={`删除第 ${index + 1} 行`}
                >
                  ×
                </button>
              </div>
            ))}
          </div>

          <div className="entry-foot">
            <div>
              <button
                type="button"
                className="secondary"
                onClick={() =>
                  setLines((current) => [
                    ...current,
                    newSalesOrderLine(
                      catalog.skus[0]?.id,
                      availableWarehouses[0]?.id,
                      catalog.units[0]?.id,
                    ),
                  ])
                }
              >
                + 添加商品行
              </button>
              <label className="entry-note">
                <span>业务备注</span>
                <input
                  value={businessNote}
                  maxLength={500}
                  onChange={(event) => setBusinessNote(event.target.value)}
                  placeholder="可选"
                />
              </label>
            </div>
            <dl className="entry-total">
              <div>
                <dt>价税前</dt>
                <dd>¥ {formatAmount(totals.subtotal)}</dd>
              </div>
              <div>
                <dt>折扣</dt>
                <dd>− ¥ {formatAmount(totals.discount)}</dd>
              </div>
              <div>
                <dt>税额</dt>
                <dd>¥ {formatAmount(totals.tax)}</dd>
              </div>
              <div className="grand">
                <dt>订单合计</dt>
                <dd>CNY {formatAmount(totals.gross)}</dd>
              </div>
            </dl>
          </div>
          {notice && <p className="entry-notice">{notice}</p>}
          <button
            className="entry-save"
            type="submit"
            disabled={busy || !ready}
          >
            {busy ? "正在保存…" : orderId ? "保存修改" : "保存销售订单草稿"}
          </button>
        </form>
      )}
    </section>
  );
}

function Field({
  label,
  children,
}: React.PropsWithChildren<{ label: string }>) {
  const id = React.useId();
  const control = React.Children.only(children) as React.ReactElement<{
    id?: string;
  }>;
  return (
    <div className="entry-field">
      <label htmlFor={id}>{label}</label>
      {React.cloneElement(control, { id })}
    </div>
  );
}

function option(item: MasterDataRecord) {
  return (
    <option value={item.id} key={item.id}>
      {item.code} · {item.name}
    </option>
  );
}

async function loadMaster(resource: string) {
  const response = await request<MasterDataList>(
    `/api/v1/master-data/${resource}?limit=200`,
  );
  return response.items.filter((item) => item.status === "active");
}

function lineLabel(
  field: "quantity" | "unitPrice" | "discountAmount" | "taxRate",
) {
  return {
    quantity: "数量",
    unitPrice: "单价",
    discountAmount: "折扣",
    taxRate: "税率 %",
  }[field];
}

function amount(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function today() {
  const value = new Date();
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
}
