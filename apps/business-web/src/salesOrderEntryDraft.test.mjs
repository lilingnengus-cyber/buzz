import assert from "node:assert/strict";
import test from "node:test";
import {
  isCompleteSalesOrderLine,
  newSalesOrderLine,
} from "./salesOrderEntryDraft.ts";

function completeLine(unitPrice) {
  return {
    ...newSalesOrderLine("sku-1", "warehouse-1", "uom-1"),
    unitPrice,
  };
}

test("requires an operator or agent to enter the sales unit price", () => {
  const line = completeLine("");

  assert.equal(line.unitPrice, "");
  assert.equal(isCompleteSalesOrderLine(line), false);
  assert.equal(isCompleteSalesOrderLine({ ...line, unitPrice: "   " }), false);
});

test("accepts explicit zero-price and positive-price sales lines", () => {
  assert.equal(isCompleteSalesOrderLine(completeLine("0")), true);
  assert.equal(isCompleteSalesOrderLine(completeLine("1.00")), true);
});

test("rejects invalid, negative, and zero-quantity sales lines", () => {
  assert.equal(isCompleteSalesOrderLine(completeLine("not-a-number")), false);
  assert.equal(isCompleteSalesOrderLine(completeLine("-1")), false);
  assert.equal(
    isCompleteSalesOrderLine({ ...completeLine("1"), quantity: "0" }),
    false,
  );
});
