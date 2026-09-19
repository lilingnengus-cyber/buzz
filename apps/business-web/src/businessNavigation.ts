export type Section =
  | "agentQuery"
  | "dashboard"
  | "quality"
  | "incidents"
  | "trends"
  | "crm"
  | "coreData"
  | "productData"
  | "numbering"
  | "sales"
  | "shipments"
  | "salesReturns"
  | "purchaseReturns"
  | "inventoryOpening"
  | "inventory"
  | "receivables"
  | "receipts"
  | "purchasing"
  | "goodsReceipts"
  | "payables"
  | "supplierPayments"
  | "profits"
  | "profitability"
  | "adjustments"
  | "reports";
type NavItem = { id: Section; label: string; index: string };

export const NAV_GROUPS: Array<{
  id: string;
  label: string;
  index: string;
  items: NavItem[];
}> = [
  {
    id: "control",
    label: "经营控制",
    index: "01",
    items: [
      { id: "dashboard", label: "经营驾驶舱", index: "OPS" },
      { id: "quality", label: "数据质量", index: "DQ" },
      { id: "incidents", label: "异常处置", index: "INC" },
      { id: "trends", label: "日报与趋势", index: "TRD" },
    ],
  },
  {
    id: "master-data",
    label: "基础资料",
    index: "02",
    items: [
      { id: "coreData", label: "核心数据", index: "MDM" },
      { id: "productData", label: "商品数据", index: "PDM" },
      { id: "numbering", label: "编码规则", index: "NUM" },
    ],
  },
  {
    id: "workflows",
    label: "业务闭环",
    index: "03",
    items: [
      { id: "crm", label: "售前 CRM", index: "CRM" },
      { id: "sales", label: "销售订单闭环", index: "O2C" },
      { id: "inventory", label: "库存台账", index: "INV" },
      { id: "purchasing", label: "采购订单闭环", index: "P2P" },
    ],
  },
  {
    id: "analysis",
    label: "经营分析",
    index: "04",
    items: [
      { id: "profits", label: "订单真实利润", index: "P&L" },
      { id: "profitability", label: "多维盈利分析", index: "DIM" },
      { id: "adjustments", label: "经营费用归集", index: "ADJ" },
      { id: "reports", label: "管理利润报表", index: "RPT" },
    ],
  },
];
export const NAV = NAV_GROUPS.flatMap((group) => group.items);
