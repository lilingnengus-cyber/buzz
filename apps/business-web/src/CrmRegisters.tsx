import React from "react";
import { LEAD_STATUSES, leadLink, type Lead } from "./crmLeads";
import { CrmLeadFollowupCreate } from "./CrmLeadFollowupCreate";
import { CrmFollowupCreate } from "./CrmFollowupCreate";
import { CrmDirectoryPage } from "./CrmDirectoryPage";
import { request } from "./api";
import { CrmDrawer } from "./CrmDrawer";
import { CrmFollowupDetail } from "./CrmRecordDetails";
import { CrmAccountPicker } from "./CrmDirectoryFields";
import { CRM_STAGES, localDate, type CrmAccount, type Followup } from "./crm";
import "./crm.css";
import "./crm-opportunities.css";
import "./crm-registers.css";

type Note = Followup & {
  leadId?: string; leadStatus?: Lead["status"]; sourceLeadId?: string;
  opportunityId: string;
  opportunityTitle: string;
  companyName: string;
  contactName: string;
};
type Register = { items: Note[]; hasMore: boolean };
const opportunityLink = (id: string) =>
  `/#crm?opportunity=${encodeURIComponent(id)}`;

export function CrmRegisters({ view }: { view: "followups" | "contacts" }) {
  return view === "contacts" ? <CrmDirectoryPage /> : <CrmHistoryRegister />;
}
function CrmHistoryRegister() {
  const [creatingLead, setCreatingLead] = React.useState(false);
  const [creating, setCreating] = React.useState(false);
  const [canManage, setCanManage] = React.useState(false);
  const [notice, setNotice] = React.useState("");
  const [permissionError, setPermissionError] = React.useState("");
  const [permissionRevision, setPermissionRevision] = React.useState(0);
  const view = "followups";
  const title = "跟进记录";
  const [selected, setSelected] = React.useState<Note | null>(null);
  const [account, setAccount] = React.useState<CrmAccount | null>(null);
  const [due, setDue] = React.useState("");
  const [query, setQuery] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setCanManage(false);
    setPermissionError("");
    request<{ canManage: boolean }>("/api/v1/crm/opportunities?offset=0")
      .then((r) => {
        if (active) setCanManage(r.canManage);
      })
      .catch((reason) => {
        if (active)
          setPermissionError(
            reason instanceof Error ? reason.message : "权限读取失败",
          );
      });
    return () => {
      active = false;
    };
  }, [revision, permissionRevision]);
  const [data, setData] = React.useState<Register>({
    items: [],
    hasMore: false,
  });
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    const timer = setTimeout(() => {
      const params = new URLSearchParams({
        query: query.trim(),
        offset: String(offset),
      });
      if (due) {
        params.set("followup", due);
        params.set("today", localDate());
      }
      if (account) params.set("accountId", account.id);
      request<Register>(`/api/v1/crm/${view}?${params}`)
        .then((result) => {
          if (active) setData(result);
        })
        .catch((e) => {
          if (active) {
            setData({ items: [], hasMore: false });
            setError(e instanceof Error ? e.message : `${title}加载失败`);
          }
        })
        .finally(() => {
          if (active) setLoading(false);
        });
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [view, query, offset, revision, title, account, due]);
  return (
    <section className="crm-page crm-opportunities crm-history-page">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>{title}</h1>
          <p className="crm-hint">
            统一查看线索与商机的沟通历史。
          </p>
        </div>
        {canManage && <button onClick={() => setCreatingLead(true)}>线索跟进</button>}
        {canManage && (
          <button className="primary" onClick={() => setCreating(true)}>
            新建跟进
          </button>
        )}
      </header>
      {notice && <p role="status">{notice}</p>}
      {permissionError && (
        <p role="alert" className="crm-error">
          跟进录入权限读取失败：{permissionError}{" "}
          <button onClick={() => setPermissionRevision((v) => v + 1)}>
            重试录入权限
          </button>
        </p>
      )}
      {creatingLead && <CrmDrawer title="线索跟进" onClose={() => setCreatingLead(false)}><CrmLeadFollowupCreate onSaved={async () => {setCreatingLead(false);setRevision(v=>v+1);setNotice("线索跟进已保存");}}/></CrmDrawer>}
      {creating && (
        <CrmDrawer title="新建跟进" onClose={() => setCreating(false)}>
          <CrmFollowupCreate
            onSaved={async () => {
              setCreating(false);
              setQuery("");
              setOffset(0);
              setRevision((v) => v + 1);
              setNotice("跟进已保存，关联商机已更新。");
            }}
          />
        </CrmDrawer>
      )}
      <div className="crm-toolbar crm-filter-toolbar crm-followup-filters">
        <label className="crm-search">
          搜索{title}
          <input
            type="search"
            maxLength={160}
            value={query}
            placeholder="公司、商机、联系人或沟通内容"
            onChange={(e) => {
              setQuery(e.target.value);
              setOffset(0);
            }}
          />
        </label>
        <label>
          跟进安排
          <select
            aria-label="跟进安排"
            value={due}
            onChange={(event) => {
              setDue(event.target.value);
              setOffset(0);
            }}
          >
            <option value="open">进行中</option>
            <option value="overdue">逾期</option>
            <option value="today">今天</option>
            <option value="upcoming">未来七天</option>
            <option value="unscheduled">未安排</option>
            <option value="">全部</option>
          </select>
        </label>
        <CrmAccountPicker
          filter
          value={account}
          onChange={(value) => {
            setAccount(value);
            setOffset(0);
          }}
        />
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
      {due && (
        <p className="crm-hint">
          按关联线索或商机当前的跟进安排筛选；记录内日期保留当时安排，历史记录不代表当前待办。
        </p>
      )}
      {error && (
        <p role="alert" className="crm-error">
          {error}{" "}
          <button onClick={() => setRevision((v) => v + 1)}>重新加载</button>
        </p>
      )}
      <div className="crm-register" aria-busy={loading}>
        <div className="crm-list-caption">
          <strong>沟通历史</strong>
          <span>
            {loading ? "加载中…" : `本页 ${data.items.length} 条记录`} ·
            点击记录查看详情
          </span>
        </div>
        <div
          className="crm-followup-scroll"
          role="region"
          aria-label="跟进记录字段列表"
          tabIndex={0}
        >
          <div
            className="crm-register-columns crm-followup-grid"
            aria-hidden="true"
          >
            {[
              "线索 / 商机",
              "客户",
              "联系人",
              "阶段",
              "沟通内容",
              "下一步",
              "跟进日期",
              "记录人",
              "记录时间",
              "流失原因",
            ].map((label) => (
              <span key={label}>{label}</span>
            ))}
          </div>
          {loading ? (
            <p role="status">正在加载{title}…</p>
          ) : !error && !data.items.length ? (
            <div className="crm-empty">
              <h2>{query ? "没有符合条件的记录" : `还没有${title}`}</h2>
              <p>
                {query
                  ? "调整搜索条件后重试。"
                  : "点击新建跟进，选择商机并记录客户沟通。"}
              </p>
              <a href="/#crm">查看商机</a>
            </div>
          ) : (
            data.items.map((item) => (
              <article
                className="crm-register-card crm-clickable-record crm-followup-grid"
                key={item.id}
                role="button"
                tabIndex={0}
                aria-haspopup="dialog"
                aria-label={`查看跟进：${item.opportunityTitle}`}
                onClick={(event) => {
                  if (
                    !(
                      event.target instanceof Element &&
                      event.target.closest("a, button")
                    )
                  )
                    setSelected(item);
                }}
                onKeyDown={(event) => {
                  if (
                    event.target === event.currentTarget &&
                    ["Enter", " "].includes(event.key)
                  ) {
                    event.preventDefault();
                    setSelected(item);
                  }
                }}
              >
                <div className="crm-register-cell">
                  <a href={item.leadId ? leadLink(item.leadId) : opportunityLink(item.opportunityId)}>
                    {item.opportunityTitle}
                  </a><small>{item.leadId ? "线索" : item.sourceLeadId ? "线索转入" : "商机"}</small>
                </div>
                <div className="crm-register-cell">{item.companyName}</div>
                <div className="crm-register-cell">
                  {item.contactName || "未填写"}
                </div>
                <div className="crm-register-cell">
                  <span className={`crm-stage crm-stage-${item.stage}`}>
                    {item.leadStatus ? LEAD_STATUSES[item.leadStatus] : CRM_STAGES[item.stage]}
                  </span>
                </div>
                <div className="crm-register-cell">
                  <p className="crm-note-preview">{item.note}</p>
                </div>
                <div className="crm-register-cell">
                  {item.nextAction || "未安排"}
                </div>
                <div className="crm-register-cell">
                  {item.nextFollowUp ? (
                    <time dateTime={item.nextFollowUp}>
                      {item.nextFollowUp}
                    </time>
                  ) : (
                    "未安排"
                  )}
                </div>
                <div className="crm-register-cell">{item.authorName}</div>
                <div className="crm-register-cell">
                  <time dateTime={item.createdAt}>
                    {new Date(item.createdAt).toLocaleString("zh-CN", {
                      year: "numeric",
                      month: "2-digit",
                      day: "2-digit",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </time>
                </div>
                <div className="crm-register-cell">
                  {item.lossReason || "—"}
                </div>
              </article>
            ))
          )}
        </div>
      </div>
      {selected && (
        <CrmDrawer title="跟进记录详情" onClose={() => setSelected(null)}>
          <CrmFollowupDetail item={selected} />
        </CrmDrawer>
      )}
      {!error && (offset > 0 || data.hasMore) && (
        <nav className="crm-pagination" aria-label={`${title}分页`}>
          <button
            disabled={loading || offset === 0}
            onClick={() => setOffset(Math.max(0, offset - 50))}
          >
            上一页
          </button>
          <span>第 {offset / 50 + 1} 页</span>
          <button
            disabled={loading || !data.hasMore}
            onClick={() => setOffset(offset + 50)}
          >
            下一页
          </button>
        </nav>
      )}
    </section>
  );
}
