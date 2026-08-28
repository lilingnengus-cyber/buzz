export type SalesOrderLineDraft = {
  key: string;
  skuId: string;
  warehouseId: string;
  unitOfMeasureId: string;
  quantity: string;
  unitPrice: string;
  discountAmount: string;
  taxRate: string;
};

export function newSalesOrderLine(
  skuId = "",
  warehouseId = "",
  unitOfMeasureId = "",
): SalesOrderLineDraft {
  return {
    key: crypto.randomUUID(),
    skuId,
    warehouseId,
    unitOfMeasureId,
    quantity: "1",
    unitPrice: "",
    discountAmount: "0",
    taxRate: "0",
  };
}

export function isCompleteSalesOrderLine(line: SalesOrderLineDraft) {
  return Boolean(
    line.skuId &&
      line.warehouseId &&
      line.unitOfMeasureId &&
      validDecimal(line.quantity, (value) => value > 0) &&
      validDecimal(line.unitPrice, (value) => value >= 0),
  );
}

function validDecimal(value: string, predicate: (value: number) => boolean) {
  if (value.trim() === "") return false;
  const parsed = Number(value);
  return Number.isFinite(parsed) && predicate(parsed);
}
