import assert from "node:assert/strict";
import test from "node:test";

import {
  isBusinessDeepLinkCandidate,
  buildBusinessReference,
  buildBusinessUrl,
  isBusinessResource,
  parseBusinessUrl,
  resolveBusinessResource,
} from "./businessResourceResolver.ts";

const config = {
  homeUrl: "https://biz.example.com/embed/",
  origin: "https://biz.example.com",
};

for (const [name, url, type, id] of [
  [
    "CRM opportunity",
    "https://biz.example.com/embed/crm/opportunities/123e4567-e89b-12d3-a456-426614174000",
    "crm_opportunity",
    "123e4567-e89b-12d3-a456-426614174000",
  ],
  [
    "agent query",
    "https://biz.example.com/embed/agent-queries/fc84644d-43ac-462f-8a30-456e04a2e9a3",
    "agent_query",
    "fc84644d-43ac-462f-8a30-456e04a2e9a3",
  ],
  [
    "sales order",
    "https://biz.example.com/embed/sales-orders/SO-001",
    "sales_order",
    "SO-001",
  ],
  [
    "purchase order",
    "https://biz.example.com/embed/purchase-orders/PO-001",
    "purchase_order",
    "PO-001",
  ],
  [
    "customer",
    "https://biz.example.com/embed/customers/CUST-001",
    "customer",
    "CUST-001",
  ],
  [
    "report",
    "https://biz.example.com/embed/reports/profitability",
    "management_report",
    "profitability",
  ],
  [
    "anomaly",
    "https://biz.example.com/embed/anomalies/2e4ae4d4-ecf1-49e7-8522-fc7bd190688f",
    "anomaly",
    "2e4ae4d4-ecf1-49e7-8522-fc7bd190688f",
  ],
  [
    "action proposal",
    "https://biz.example.com/embed/action-proposals/AP-001",
    "action_proposal",
    "AP-001",
  ],
  [
    "work item",
    "https://biz.example.com/embed/work-items/WI-001",
    "work_item",
    "WI-001",
  ],
  [
    "approval draft",
    "https://biz.example.com/embed/approval-drafts/AD-001",
    "approval_draft",
    "AD-001",
  ],
]) {
  test(`parses a ${name} business URL`, () => {
    const resource = parseBusinessUrl(url, config);
    assert.equal(resource?.type, type);
    assert.equal(resource?.id, id);
  });
}

test("parses and rebuilds an allowlisted biz deep link", () => {
  const resource = parseBusinessUrl("biz://sales-order/SO-001", config);
  assert.deepEqual(resource, {
    version: 1,
    type: "sales_order",
    id: "SO-001",
    path: "/embed/sales-orders/SO-001",
  });
  assert.equal(buildBusinessReference(resource), "biz://sales-order/SO-001");
  assert.equal(
    buildBusinessUrl(resource, config),
    "https://biz.example.com/embed/sales-orders/SO-001",
  );
});

test("parses and rebuilds an agent query receipt deep link", () => {
  const reference = "biz://agent-query/fc84644d-43ac-462f-8a30-456e04a2e9a3";
  const resource = parseBusinessUrl(reference, config);
  assert.deepEqual(resource, {
    version: 1,
    type: "agent_query",
    id: "fc84644d-43ac-462f-8a30-456e04a2e9a3",
    path: "/embed/agent-queries/fc84644d-43ac-462f-8a30-456e04a2e9a3",
  });
  assert.equal(buildBusinessReference(resource), reference);
});

test("parses and rebuilds V6 lifecycle deep links without query data", () => {
  for (const [deepLink, type, path] of [
    ["anomaly", "anomaly", "/embed/anomalies/FIND-001"],
    ["action-proposal", "action_proposal", "/embed/action-proposals/AP-001"],
    ["work-item", "work_item", "/embed/work-items/WI-001"],
    ["approval-draft", "approval_draft", "/embed/approval-drafts/AD-001"],
  ]) {
    const reference = `biz://${deepLink}/${path.split("/").at(-1)}`;
    const resource = parseBusinessUrl(reference, config);
    assert.equal(resource?.type, type);
    assert.equal(resource?.path, path);
    assert.equal(buildBusinessReference(resource), reference);
  }
  assert.equal(
    parseBusinessUrl("biz://work-item/WI-001?token=no", config),
    null,
  );
});

test("parses B2 shipment and customer receipt deep links", () => {
  const shipment = parseBusinessUrl("biz://shipment/SHP-001", config);
  assert.equal(shipment?.type, "shipment");
  assert.equal(shipment?.path, "/embed/shipments/SHP-001");
  assert.equal(buildBusinessReference(shipment), "biz://shipment/SHP-001");
  const receipt = parseBusinessUrl("biz://customer-receipt/RCPT-001", config);
  assert.equal(receipt?.type, "customer_receipt");
  assert.equal(receipt?.path, "/embed/customer-receipts/RCPT-001");
  assert.equal(
    buildBusinessReference(receipt),
    "biz://customer-receipt/RCPT-001",
  );
});

test("parses B3 receipt and supplier payment deep links", () => {
  const receipt = parseBusinessUrl("biz://goods-receipt/GR-001", config);
  assert.equal(receipt?.type, "goods_receipt");
  assert.equal(receipt?.path, "/embed/goods-receipts/GR-001");
  const payment = parseBusinessUrl("biz://supplier-payment/PAY-001", config);
  assert.equal(payment?.type, "supplier_payment");
  assert.equal(payment?.path, "/embed/supplier-payments/PAY-001");
});

test("parses and rebuilds B4 profit deep links", () => {
  for (const [reference, type, path] of [
    [
      "biz://order-profit/SO-001",
      "order_profit",
      "/embed/order-profits/SO-001",
    ],
    [
      "biz://profit-adjustment/ADJ-001",
      "profit_adjustment",
      "/embed/profit-adjustments/ADJ-001",
    ],
    [
      "biz://management-report/RPT-001",
      "management_report",
      "/embed/management-reports/RPT-001",
    ],
  ]) {
    const resource = parseBusinessUrl(reference, config);
    assert.equal(resource?.type, type);
    assert.equal(resource?.path, path);
    assert.equal(buildBusinessReference(resource), reference);
  }
  const profitability = parseBusinessUrl(
    "biz://profitability/customer/CUST-001/2026-08",
    config,
  );
  assert.deepEqual(profitability, {
    version: 1,
    type: "profitability",
    id: "CUST-001",
    period: "2026-08",
    metadata: { dimension: "customer" },
    path: "/embed/profitability/customer/CUST-001/period/2026-08",
  });
  assert.equal(
    buildBusinessReference(profitability),
    "biz://profitability/customer/CUST-001/2026-08",
  );
});

test("parses S1 operating singleton links", () => {
  for (const [reference, type, path] of [
    [
      "biz://operations-dashboard",
      "operations_dashboard",
      "/embed/operations-dashboard",
    ],
    ["biz://data-quality", "data_quality", "/embed/data-quality"],
    [
      "biz://operating-incidents",
      "operating_incidents",
      "/embed/operating-incidents",
    ],
    ["biz://operating-trends", "operating_trends", "/embed/operating-trends"],
  ]) {
    const resource = parseBusinessUrl(reference, config);
    assert.equal(resource?.type, type);
    assert.equal(resource?.path, path);
    assert.equal(resource?.id, undefined);
    assert.equal(buildBusinessReference(resource), reference);
  }
  assert.equal(parseBusinessUrl("biz://data-quality/extra", config), null);
  assert.equal(
    parseBusinessUrl("https://biz.example.com/embed/data-quality-evil", config)
      ?.type,
    "generic",
  );
});

test("parses and rebuilds conversation entry links", () => {
  for (const [reference, type, path] of [
    [
      "biz://sales-order-entry",
      "sales_order_entry",
      "/embed/entries/sales-order",
    ],
    ["biz://shipment-entry", "shipment_entry", "/embed/entries/shipment"],
    [
      "biz://purchase-order-entry",
      "purchase_order_entry",
      "/embed/entries/purchase-order",
    ],
    [
      "biz://goods-receipt-entry",
      "goods_receipt_entry",
      "/embed/entries/goods-receipt",
    ],
    [
      "biz://customer-receipt-entry",
      "customer_receipt_entry",
      "/embed/entries/customer-receipt",
    ],
    [
      "biz://supplier-payment-entry",
      "supplier_payment_entry",
      "/embed/entries/supplier-payment",
    ],
  ]) {
    const resource = parseBusinessUrl(reference, config);
    assert.deepEqual(resource, { version: 1, type, path });
    assert.equal(buildBusinessReference(resource), reference);
    assert.equal(
      buildBusinessUrl(resource, config),
      `https://biz.example.com${path}`,
    );
  }
  assert.equal(parseBusinessUrl("biz://sales-order-entry/extra", config), null);
  assert.equal(
    parseBusinessUrl("biz://sales-order-entry?customer=CUST-1", config),
    null,
  );
});

test("parses server-generated receivable and payable business links", () => {
  const receivable = parseBusinessUrl(
    "biz://customer/CUST-001/receivables",
    config,
  );
  assert.deepEqual(receivable, {
    version: 1,
    type: "receivable",
    id: "CUST-001",
    path: "/embed/receivables/CUST-001",
  });
  assert.equal(
    buildBusinessReference(receivable),
    "biz://customer/CUST-001/receivables",
  );
  const payable = parseBusinessUrl("biz://supplier/SUP-001/payables", config);
  assert.equal(payable?.type, "payable");
  assert.equal(payable?.path, "/embed/payables/supplier/SUP-001");
  assert.equal(
    buildBusinessReference(payable),
    "biz://supplier/SUP-001/payables",
  );
});

test("rejects unsafe deep links, traversal, schemes, and origins", () => {
  for (const value of [
    "biz://unknown/SO-001",
    "biz://sales-order/../../admin",
    "biz://sales-order/javascript:alert(1)",
    "https://biz.example.com/embed/%2e%2e/admin",
    "https://evil.example/embed/sales-orders/SO-001",
    "javascript:alert(1)",
    "data:text/html,hello",
  ]) {
    assert.equal(parseBusinessUrl(value, config), null, value);
  }
});

test("validates structured resources and rejects sensitive metadata", () => {
  const resource = {
    version: 1,
    type: "invoice",
    id: "INV-1",
    path: "/embed/invoices/INV-1",
    metadata: { source: "agent" },
  };
  assert.equal(isBusinessResource(resource), true);
  assert.deepEqual(resolveBusinessResource(resource, config), resource);
  assert.equal(
    isBusinessResource({ ...resource, metadata: { accessToken: "nope" } }),
    false,
  );
  assert.equal(
    isBusinessResource({ ...resource, path: "/embed/invoices/../admin" }),
    false,
  );
});

test("opens an inventory opening at its exact system detail", () => {
  const opening = parseBusinessUrl("biz://inventory-opening/OPEN-001", config);
  assert.equal(opening?.type, "inventory_opening");
  assert.equal(opening?.path, "/embed/inventory-openings/OPEN-001");
});

for (const kind of ["sales", "purchase"]) {
  test(`${kind} return opens its own detail page`, () => {
    const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
    const resource = parseBusinessUrl(`biz://${kind}-return/${id}`, config);
    assert.equal(resource?.type, `${kind}_return`);
    assert.equal(resource?.path, `/embed/${kind}-returns/${id}`);
    assert.equal(resource?.id, id);
    assert.equal(
      parseBusinessUrl(
        `https://evil.example/embed/${kind}-returns/${id}`,
        config,
      ),
      null,
    );
  });
}

test("inventory count reference opens the exact count detail", () => {
  const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
  const resource = resolveBusinessResource(
    `biz://inventory-count/${id}`,
    config,
  );
  assert.equal(resource?.type, "inventory_count");
  assert.equal(resource?.id, id);
  assert.equal(resource?.path, `/embed/inventory-counts/${id}`);
  assert.equal(
    buildBusinessUrl(resource, config),
    `${config.origin}/embed/inventory-counts/${id}`,
  );
  assert.equal(buildBusinessReference(resource), `biz://inventory-count/${id}`);
});

test("CRM chat reference opens the exact opportunity", () => {
  const resource = resolveBusinessResource(
    "biz://crm-opportunity/123e4567-e89b-12d3-a456-426614174000",
    config,
  );
  assert.equal(
    resource?.path,
    "/embed/crm/opportunities/123e4567-e89b-12d3-a456-426614174000",
  );
});

test("master links bind resource kind and UUID across parsing and serialization", () => {
  const id = "123e4567-e89b-12d3-a456-426614174000";
  for (const kind of [
    "legal_entity",
    "business_unit",
    "customer",
    "supplier",
    "warehouse",
    "unit_of_measure",
    "product_category",
    "brand",
    "product",
    "sku",
    "uom_conversion",
  ]) {
    const uri = `biz://master-data/${kind}/${id}`;
    assert.equal(isBusinessDeepLinkCandidate(uri), true);
    const resource = parseBusinessUrl(uri, config);
    assert.equal(resource?.type, "master_data");
    assert.equal(buildBusinessReference(resource), uri);
    assert.equal(
      buildBusinessUrl(resource, config),
      `${config.origin}/embed/master-data/${kind}/${id}`,
    );
    assert.deepEqual(
      parseBusinessUrl(buildBusinessUrl(resource, config), config),
      resource,
    );
    assert.equal(isBusinessResource({ ...resource, id: "different" }), false);
    assert.equal(
      isBusinessResource({
        ...resource,
        metadata: { resourceType: "different" },
      }),
      false,
    );
  }
  for (const suffix of [
    `users/${id}`,
    "product/not-an-id",
    `product/${id}/extra`,
    `product/${id}?token=bad`,
  ]) {
    assert.equal(
      isBusinessDeepLinkCandidate(`biz://master-data/${suffix}`),
      false,
    );
    assert.equal(parseBusinessUrl(`biz://master-data/${suffix}`, config), null);
  }
});
