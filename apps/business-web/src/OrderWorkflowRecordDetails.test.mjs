import assert from "node:assert/strict";
import test from "node:test";
import {
  purchaseOrderDetail,
  salesOrderDetail,
  statusLabel,
} from "./OrderWorkflowRecordDetails.ts";

test("builds a readable sales-order detail record", () => {
  const detail = salesOrderDetail({
    id: "order-1",
    orderNumber: "SO-2026-001",
    legalEntityId: "entity-1",
    customerId: "customer-1",
    currency: "CNY",
    lifecycleStatus: "confirmed",
    holdStatus: "none",
    fulfillmentStatus: "partially_shipped",
    grossAmount: "1280.5",
    orderDate: "2026-08-22",
    updatedAt: "2026-08-22T08:30:00Z",
    version: 3,
  });

  assert.equal(detail.kind, "record-detail");
  assert.equal(detail.domain, "sales");
  assert.equal(detail.title, "销售订单 · SO-2026-001");
  assert.deepEqual(
    detail.fields.slice(0, 5).map(({ label, value }) => [label, value]),
    [
      ["订单编号", "SO-2026-001"],
      ["含税总额", "CNY 1,280.50"],
      ["订单状态", "已确认"],
      ["履约状态", "部分出库"],
      ["冻结状态", "正常"],
    ],
  );
});

test("keeps status labels consistent between rows and details", () => {
  assert.equal(statusLabel("fully_allocated"), "已核销");
  assert.equal(statusLabel("supplier_acknowledged"), "供应商已签收");
});

test("shows purchase-order business dimensions as readable master data", () => {
  const detail = purchaseOrderDetail({
    id: "purchase-1",
    purchaseOrderNumber: "PO-2026-001",
    legalEntityId: "legal-1",
    legalEntityCode: "LE_CN_01",
    legalEntityName: "杭州主体",
    supplierId: "supplier-1",
    supplierCode: "SU-000001",
    supplierName: "共享供应商",
    businessUnitId: "unit-1",
    businessUnitCode: "BU_CN_01",
    businessUnitName: "华东事业部",
    warehouseLabels: ["WH-000001 · 杭州仓", "WH-000002 · 上海仓"],
    currency: "CNY",
    lifecycleStatus: "draft",
    receivingStatus: "unreceived",
    grossAmount: "1",
    orderDate: "2026-10-01",
    updatedAt: "2026-10-01T03:21:00Z",
    version: 1,
  });

  assert.deepEqual(
    detail.fields.slice(5, 9).map(({ label, value }) => [label, value]),
    [
      ["法定主体", "LE_CN_01 · 杭州主体"],
      ["经营主体", "BU_CN_01 · 华东事业部"],
      ["供应商", "SU-000001 · 共享供应商"],
      ["仓库", "WH-000001 · 杭州仓；WH-000002 · 上海仓"],
    ],
  );
});
