export const CRM_STAGES = {
  new: "新线索",
  contacting: "沟通中",
  quoting: "报价中",
  won: "已成交",
  lost: "已流失",
} as const;
export type CrmStage = keyof typeof CRM_STAGES;
export type Opportunity = {
  id: string;
  legalEntityId: string;
  businessUnitId: string;
  customerId: string | null;
  title: string;
  companyName: string;
  contactName: string;
  contactDetails: string;
  stage: CrmStage;
  expectedAmountMinor: number | null;
  currency: string;
  nextAction: string;
  nextFollowUp: string | null;
  version: number;
  createdAt: string;
  updatedAt: string;
};
export type CrmOption = {
  id: string;
  name: string;
  code: string;
  resourceType: string;
  legalEntityId: string | null;
  businessUnitId: string | null;
};
export type Followup = {
  id: string;
  note: string;
  stage: CrmStage;
  nextAction: string;
  nextFollowUp: string | null;
  createdAt: string;
  authorName: string;
};
export type CrmDetail = {
  item: Opportunity;
  followups: Followup[];
  hasOlderFollowups: boolean;
};
export function localDate(date = new Date()) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
export function isDue(item: Opportunity, today = localDate()) {
  return (
    item.stage !== "won" &&
    item.stage !== "lost" &&
    !!item.nextFollowUp &&
    item.nextFollowUp <= today
  );
}
export function amountMinor(value: string): number | null {
  if (!value.trim()) return null;
  if (!/^\d+(\.\d{1,2})?$/.test(value))
    throw new Error("预计金额最多保留两位小数");
  const [whole, fraction = ""] = value.split(".");
  const amount = Number(whole) * 100 + Number(fraction.padEnd(2, "0"));
  if (!Number.isSafeInteger(amount) || amount > 999999999999)
    throw new Error("预计金额超出范围");
  return amount;
}
