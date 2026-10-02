import React from "react";
import { CrmDirectoryPage } from "./CrmDirectoryPage";
import { request } from "./api";
import { CrmDrawer } from "./CrmDrawer";
import { CrmFollowupDetail } from "./CrmRecordDetails";
import { CRM_STAGES, type Followup } from "./crm";
import "./crm.css";
import "./crm-opportunities.css";
import "./crm-registers.css";

type Note = Followup & {
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
  const view = "followups";
  const title = "跟进记录";
  const [selected, setSelected] = React.useState<Note | null>(null);
  const [query, setQuery] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [revision, setRevision] = React.useState(0);
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
  }, [view, query, offset, revision, title]);
  return (
    <section className="crm-page crm-opportunities crm-history-page">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>{title}</h1>
          <p className="crm-hint">按时间查看客户沟通，回到商机继续跟进。</p>
        </div>
      </header>
      <div className="crm-toolbar">
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
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
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
          className="crm-register-columns crm-followup-grid"
          aria-hidden="true"
        >
          <span>沟通记录 / 商机</span>
          <span>客户 / 联系人</span>
          <span>下一步</span>
          <span>跟进日期</span>
        </div>
        {loading ? (
          <p role="status">正在加载{title}…</p>
        ) : !error && !data.items.length ? (
          <div className="crm-empty">
            <h2>{query ? "没有符合条件的记录" : `还没有${title}`}</h2>
            <p>
              {query ? "调整搜索条件后重试。" : "打开商机，记录一次客户沟通。"}
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
              <div className="crm-communication">
                <div className="crm-row-top">
                  <a href={opportunityLink(item.opportunityId)}>
                    {item.opportunityTitle}
                  </a>
                  <span className={`crm-stage crm-stage-${item.stage}`}>
                    {CRM_STAGES[item.stage]}
                  </span>
                </div>
                <p className="crm-note-preview">{item.note}</p>
                {item.lossReason && (
                  <p className="crm-hint">流失原因：{item.lossReason}</p>
                )}
                <div className="crm-note-meta">
                  <strong>{item.authorName}</strong>
                  <time dateTime={item.createdAt}>
                    {new Date(item.createdAt).toLocaleString("zh-CN", {
                      month: "2-digit",
                      day: "2-digit",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </time>
                </div>
              </div>
              <div className="crm-register-cell">
                <span className="crm-mobile-label">客户 / 联系人</span>
                <strong>{item.companyName}</strong>
                <span className="crm-hint">
                  {item.contactName || "未填写联系人"}
                </span>
              </div>
              <div className="crm-register-cell">
                <span className="crm-mobile-label">下一步</span>
                <span>{item.nextAction || "未安排"}</span>
              </div>
              <div className="crm-register-cell">
                <span className="crm-mobile-label">跟进日期</span>
                {item.nextFollowUp ? (
                  <time dateTime={item.nextFollowUp}>{item.nextFollowUp}</time>
                ) : (
                  <span className="crm-hint">未安排</span>
                )}

              </div>
            </article>
          ))
        )}
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
