import { NAV, type Section } from "./businessNavigation";

export const WORKFLOW_NAV_ALIASES: Partial<Record<Section, Section>> = {
  shipments: "sales",
  receivables: "sales",
  receipts: "sales",
  goodsReceipts: "purchasing",
  payables: "purchasing",
  supplierPayments: "purchasing",
};

export function route(): { section: Section; id?: string; embed: boolean } {
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
  if (clean === "/crm/followups") return { section: "crmFollowups", embed };
  if (clean === "/crm/contacts") return { section: "crmContacts", embed };
  if (clean === "/crm") return { section: "crm", embed };
  if (clean === "/core-data") return { section: "coreData", embed };
  if (clean === "/product-data") return { section: "productData", embed };
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
