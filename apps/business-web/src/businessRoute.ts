import type { CoreMasterType, ProductMasterType } from "./api";
import { NAV, type Section } from "./businessNavigation";

export const WORKFLOW_NAV_ALIASES: Partial<Record<Section, Section>> = {
  shipments: "goodsOrders",
  receivables: "goodsOrders",
  receipts: "goodsOrders",
  serviceProjects: "serviceOrders",
  serviceDeliverables: "serviceOrders",
  goodsReceipts: "purchasing",
  payables: "purchasing",
  supplierPayments: "purchasing",
};

export function route(): { section: Section; id?: string; masterType?: CoreMasterType | ProductMasterType; embed: boolean } {
  const path = window.location.pathname;
  const embed = path.startsWith("/embed/");
  const clean = path.replace(/^\/embed/, "");
  if (clean === "/operations-dashboard") return { section: "dashboard", embed };
  if (clean === "/data-quality") return { section: "quality", embed };
  if (clean === "/operating-incidents") return { section: "incidents", embed };
  if (clean === "/operating-trends") return { section: "trends", embed };
  const agentQuery = clean.match(/^\/agent-queries\/([^/]+)$/);
  if (agentQuery)
    return {
      section: "agentQuery",
      id: decodeURIComponent(agentQuery[1]),
      embed,
    };
  const lead = clean.match(/^\/crm\/leads\/([^/]+)$/);
  if (lead) return { section: "crmLeads", id: decodeURIComponent(lead[1]), embed };
  const opportunity = clean.match(/^\/crm\/opportunities\/([^/]+)$/);
  if (opportunity) return { section: "crm", id: decodeURIComponent(opportunity[1]), embed };
  if (clean === "/crm/followups") return { section: "crmFollowups", embed };
  if (clean === "/crm/contacts") return { section: "crmContacts", embed };
  if (clean === "/crm/leads") return { section: "crmLeads", embed };
  if (clean === "/crm") return { section: "crm", embed };
  const master = clean.match(/^\/(customers|suppliers|warehouses|products|skus)\/([^/]+)$/);
  if (master) {
    const types = { customers: "customer", suppliers: "supplier", warehouses: "warehouse", products: "product", skus: "sku" } as const;
    const masterType = types[master[1] as keyof typeof types];
    return { section: masterType === "product" || masterType === "sku" ? "productData" : "coreData", masterType, id: decodeURIComponent(master[2]), embed };
  }
  if (clean === "/core-data") return { section: "coreData", embed };
  if (clean === "/product-data") return { section: "productData", embed };
  if (clean === "/preferences") return { section: "preferences", embed };
  const patterns: Array<[Section, RegExp]> = [
    ["sales", /^\/(?:sales-orders|sales\/orders)\/([^/]+)$/],
    ["shipments", /^\/shipments\/([^/]+)$/],
    ["inventory", /^\/inventory\/([^/]+)$/],
    ["receivables", /^\/receivables\/(?:customer\/)?([^/]+)$/],
    ["receipts", /^\/customer-receipts\/([^/]+)$/],
    ["purchasing", /^\/purchase-orders\/([^/]+)$/],
    ["goodsReceipts", /^\/goods-receipts\/([^/]+)$/],
    ["payables", /^\/payables\/supplier\/([^/]+)$/],
    ["supplierPayments", /^\/supplier-payments\/([^/]+)$/],
    ["profits", /^\/order-profits\/([^/]+)$/],
    ["adjustments", /^\/profit-adjustments\/([^/]+)$/],
    ["reports", /^\/management-reports\/([^/]+)$/],
    [
      "profitability",
      /^\/profitability\/(?:customer|sku|brand|salesperson)\/([^/]+)\/period\/\d{4}-\d{2}$/,
    ],
  ];
  for (const [section, pattern] of patterns) {
    const match = clean.match(pattern);
    if (match) return { section, id: decodeURIComponent(match[1]), embed };
  }
  const [hashSection, hashQuery = ""] = window.location.hash
    .slice(1)
    .split("?");
  if (hashSection === "crmLeads") return { section: "crmLeads", id: new URLSearchParams(hashQuery).get("lead") ?? undefined, embed };
  if (hashSection === "crm")
    return {
      section: "crm",
      id: new URLSearchParams(hashQuery).get("opportunity") ?? undefined,
      embed,
    };
  const fromHash = hashSection as Section;
  return {
    section: WORKFLOW_NAV_ALIASES[fromHash]
      ? WORKFLOW_NAV_ALIASES[fromHash]
      : NAV.some((item) => item.id === fromHash)
        ? fromHash
        : "dashboard",
    embed,
  };
}
