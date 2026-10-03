export type Section =
  | "agentQuery"
  | "dashboard"
  | "quality"
  | "incidents"
  | "trends"
  | "crmLeads"
  | "crm"
  | "crmFollowups"
  | "crmContacts"
  | "coreData"
  | "productData"
  | "numbering"
  | "serviceProjects"
  | "serviceDeliverables"
  | "goodsOrders"
  | "serviceOrders"
  | "sales"
  | "shipments"
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
  | "reports"
  | "preferences";
type NavItem = { id: Section; label: string };

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
      { id: "dashboard", label: "经营驾驶舱" },
      { id: "quality", label: "数据质量" },
      { id: "incidents", label: "异常处置" },
      { id: "trends", label: "日报与趋势" },
    ],
  },
  {
    id: "master-data",
    label: "基础资料",
    index: "02",
    items: [
      { id: "coreData", label: "核心数据" },
      { id: "productData", label: "商品数据" },
      { id: "numbering", label: "编码规则" },
    ],
  },
  {
    id: "crm",
    label: "售前 CRM",
    index: "03",
    items: [
      { id: "crmLeads", label: "线索" },
      { id: "crm", label: "商机" },
      { id: "crmFollowups", label: "跟进记录" },
      { id: "crmContacts", label: "联系人" },
    ],
  },
  {
    id: "workflows",
    label: "业务闭环",
    index: "04",
    items: [
      { id: "sales", label: "销售订单" },
      { id: "goodsOrders", label: "商品订单闭环" },
      { id: "serviceOrders", label: "服务订单闭环" },
      { id: "purchasing", label: "采购订单闭环" },
      { id: "inventory", label: "库存台账" },
    ],
  },
  {
    id: "analysis",
    label: "经营分析",
    index: "05",
    items: [
      { id: "profits", label: "订单真实利润" },
      { id: "profitability", label: "多维盈利分析" },
      { id: "adjustments", label: "经营费用归集" },
      { id: "reports", label: "管理利润报表" },
    ],
  },
  {
    id: "personal",
    label: "个人设置",
    index: "06",
    items: [{ id: "preferences", label: "默认经营主体" }],
  },
];
export const NAV = NAV_GROUPS.flatMap((group) => group.items);
