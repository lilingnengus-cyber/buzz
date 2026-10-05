export const LEAD_SOURCES = ["官网", "转介绍", "个人开发", "天眼查导入"];
export const LEAD_STATUSES = {
  new: "待筛选",
  contacting: "跟进中",
  converted: "已转商机",
  disqualified: "已淘汰",
} as const;
export type Lead = {
  id: string;
  title: string;
  companyName: string;
  contactName: string;
  contactDetails: string;
  source: string;
  summary: string;
  nextAction: string;
  nextFollowUp: string | null;
  status: keyof typeof LEAD_STATUSES;
  disqualificationReason: string;
  customerId: string | null;
  ownerUserId: string;
  ownerName: string;
  version: number;
  convertedOpportunityId: string | null;
  createdAt: string;
  updatedAt: string;
};
export type LeadNote = {
  id: string;
  note: string;
  disqualificationReason?: string;
  status: Lead["status"];
  nextAction: string;
  nextFollowUp: string | null;
  createdAt: string;
  authorName: string;
};
export type LeadDetail = {
  canManage?: boolean;
  item: Lead;
  followups: LeadNote[];
  hasMore: boolean;
  duplicates: { id: string; title: string }[];
};
export const leadLink = (id: string) =>
  `/#crmLeads?lead=${encodeURIComponent(id)}`;
