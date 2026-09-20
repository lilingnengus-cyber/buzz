export function operatingMetricUnavailable(reason: string | undefined): string {
  if (reason === "not_attributable_to_selected_warehouses")
    return "不可按仓库拆分";
  if (reason === "not_attributable_to_selected_business_units")
    return "不可按业务单元拆分";
  if (reason === "not_attributable_to_selected_legal_entities")
    return "不可按法人拆分";
  return "不可用";
}
