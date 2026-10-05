import { LEAD_STATUSES, type Lead } from "./crmLeads";

const columns = ["线索名称", "公司名称", "联系人", "联系方式", "线索来源", "需求摘要", "负责人", "状态", "淘汰原因", "已转商机", "创建时间", "更新时间"];
function timestamp(value: string) {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString("zh-CN", { hour12: false });
}

export function CrmLeadTable({ items, onOpen }: { items: Lead[]; onOpen: (id: string) => void }) {
  return <div className="crm-lead-table-scroll" role="region" aria-label="线索完整资料" tabIndex={0}>
    <table className="crm-lead-table">
      <caption className="crm-lead-table-caption">线索资料，点击记录查看与编辑</caption>
      <thead><tr>{columns.map(label => <th key={label} scope="col">{label}</th>)}</tr></thead>
      <tbody>{items.map(item => <tr key={item.id} className="crm-lead-row" onClick={() => onOpen(item.id)}>
        <td><button type="button" className="crm-lead-title" onClick={event => { event.stopPropagation(); onOpen(item.id); }}>{item.title}</button></td>
        <td>{item.companyName || "未填写"}</td>
        <td>{item.contactName || "未填写"}</td>
        <td>{item.contactDetails || "未填写"}</td>
        <td>{item.source || "未填写"}</td>
        <td className="crm-lead-summary">{item.summary || "未填写"}</td>
        <td>{item.ownerName || "未填写"}</td>
        <td>{LEAD_STATUSES[item.status]}</td>
        <td className="crm-lead-summary">{item.disqualificationReason || "—"}</td>
        <td>{item.convertedOpportunityId ? <a href={`/#crm?opportunity=${encodeURIComponent(item.convertedOpportunityId)}`} onClick={event => event.stopPropagation()}>查看商机</a> : "—"}</td>
        <td>{timestamp(item.createdAt)}</td>
        <td>{timestamp(item.updatedAt)}</td>
      </tr>)}</tbody>
    </table>
  </div>;
}
