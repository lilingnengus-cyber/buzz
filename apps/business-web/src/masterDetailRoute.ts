export const MASTER_LABELS: Record<string, string> = {
  legal_entity: "法定主体",
  business_unit: "业务单元",
  customer: "客户",
  supplier: "供应商",
  warehouse: "仓库",
  unit_of_measure: "计量单位",
  product_category: "商品分类",
  brand: "品牌",
  product: "商品",
  sku: "SKU",
  uom_conversion: "单位换算",
};
const CORE = new Set([
  "legal_entity",
  "business_unit",
  "customer",
  "supplier",
  "warehouse",
]);
export function masterDetailRoute(value: string) {
  const match = value.match(
    /^([a-z_]+)\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i,
  );
  if (!match || !Object.hasOwn(MASTER_LABELS, match[1])) return null;
  return {
    kind: match[1],
    id: match[2],
    family: CORE.has(match[1]) ? "core" : "product",
    label: MASTER_LABELS[match[1]],
  };
}
