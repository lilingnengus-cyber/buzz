import assert from "node:assert/strict";
import test from "node:test";
import {
  goodsReceiptDetail,
  payableDetail,
  receiptDetail,
  receivableDetail,
  returnDetail,
  salesOrderDetail,
  shipmentDetail,
  statusLabel,
} from "./OrderWorkflowRecordDetails.ts";

test("builds a readable sales-order detail record", () => {
  const detail = salesOrderDetail({
    id: "order-1",
    orderNumber: "SO-2026-001",
    legalEntityId: "entity-1",
    businessUnitId: "unit-1",
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
  assert.deepEqual(detail.assignments, {
    legalEntityId: "entity-1",
    businessUnitId: "unit-1",
  });
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

test("shows settlement units as a multi-assignment derived from allocations", () => {
  const detail = receiptDetail({
    id: "receipt-1",
    receiptNumber: "RCPT-001",
    legalEntityId: "entity-1",
    businessUnitIds: ["unit-1", "unit-2"],
    customerId: "customer-1",
    currency: "CNY",
    receiptDate: "2026-09-25",
    amount: "300",
    allocatedAmount: "300",
    unappliedAmount: "0",
    status: "fully_allocated",
    updatedAt: "2026-09-25T08:30:00Z",
    version: 3,
  });

  assert.deepEqual(detail.assignments, {
    legalEntityId: "entity-1",
    businessUnitIds: ["unit-1", "unit-2"],
    businessUnitFallback: "待核销归属",
  });
  assert.equal(
    detail.fields.some(({ label }) => label === "法定主体 ID"),
    false,
  );
});

test("inherits return assignments from its source order", () => {
  const detail = returnDetail(
    {
      id: "return-1",
      returnNumber: "SRET-001",
      legalEntityId: "entity-1",
      businessUnitId: "unit-1",
      sourceId: "shipment-1",
      orderId: "order-1",
      partnerId: "customer-1",
      warehouseId: "warehouse-1",
      returnDate: "2026-09-25",
      currency: "CNY",
      reasonCode: "QUALITY_ISSUE",
      amount: "10",
      status: "confirmed",
      workflowStatus: "pending",
      version: 2,
      updatedAt: "2026-09-25T08:30:00Z",
    },
    "sales",
  );

  assert.deepEqual(detail.assignments, {
    legalEntityId: "entity-1",
    businessUnitId: "unit-1",
  });
});

test("shows source-order assignments across fulfillment documents", () => {
  const assignments = {
    legalEntityId: "entity-1",
    businessUnitId: "unit-1",
  };
  const documents = [
    shipmentDetail({
      id: "shipment-1",
      shipmentNumber: "SHP-001",
      salesOrderId: "order-1",
      ...assignments,
      warehouseId: "warehouse-1",
      shipmentDate: "2026-09-25",
      status: "confirmed",
      confirmedAt: "2026-09-25T08:30:00Z",
      updatedAt: "2026-09-25T08:30:00Z",
      version: 2,
    }),
    receivableDetail({
      id: "receivable-1",
      receivableNumber: "AR-001",
      ...assignments,
      customerId: "customer-1",
      salesOrderId: "order-1",
      shipmentId: "shipment-1",
      currency: "CNY",
      originalAmount: "100",
      settledAmount: "0",
      openAmount: "100",
      dueDate: "2026-10-25",
      status: "open",
      isOverdue: false,
      overdueDays: 0,
      updatedAt: "2026-09-25T08:30:00Z",
      version: 1,
    }),
    goodsReceiptDetail({
      id: "receipt-1",
      goodsReceiptNumber: "GR-001",
      purchaseOrderId: "order-1",
      ...assignments,
      supplierId: "supplier-1",
      warehouseId: "warehouse-1",
      receiptDate: "2026-09-25",
      status: "confirmed",
      currency: "CNY",
      grossAmount: "100",
      inventoryCostAmount: "100",
      updatedAt: "2026-09-25T08:30:00Z",
      version: 2,
    }),
    payableDetail({
      id: "payable-1",
      payableNumber: "AP-001",
      ...assignments,
      supplierId: "supplier-1",
      purchaseOrderId: "order-1",
      goodsReceiptId: "receipt-1",
      currency: "CNY",
      originalAmount: "100",
      settledAmount: "0",
      openAmount: "100",
      dueDate: "2026-10-25",
      status: "open",
      isOverdue: false,
      overdueDays: 0,
      updatedAt: "2026-09-25T08:30:00Z",
      version: 1,
    }),
  ];

  for (const detail of documents) {
    assert.deepEqual(detail.assignments, assignments);
    assert.equal(detail.fields.some(({ label }) => label === "法定主体 ID"), false);
  }
});

test("keeps status labels consistent between rows and details", () => {
  assert.equal(statusLabel("fully_allocated"), "已核销");
  assert.equal(statusLabel("supplier_acknowledged"), "供应商已签收");
});
