import React from "react";
import { request } from "./api";
import { CRM_STAGES, type Followup } from "./crm";
import "./crm.css";

type Note = Followup & {
  opportunityId: string;
  opportunityTitle: string;
  companyName: string;
  contactName: string;
};
type Contact = {
  companyName: string;
  contactName: string;
  contactDetails: string;
  opportunities: { id: string; title: string }[];
};
type Register = { items: (Note | Contact)[]; hasMore: boolean };
const opportunityLink = (id: string) =>
  `/#crm?opportunity=${encodeURIComponent(id)}`;

export function CrmRegisters({ view }: { view: "followups" | "contacts" }) {
  const contacts = view === "contacts";
  const title = contacts ? "客户联系人" : "跟进记录";
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
    <section className="crm-page">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>{title}</h1>
          <p className="crm-hint">
            {contacts
              ? "汇总商机中的联系人，查看联系方式与关联商机。"
              : "按时间查看客户沟通，回到商机继续跟进。"}
          </p>
        </div>
      </header>
      <div className="crm-toolbar">
        <label className="crm-search">
          搜索{title}
          <input
            type="search"
            maxLength={160}
            value={query}
            placeholder={
              contacts ? "公司、姓名或联系方式" : "公司、商机、联系人或沟通内容"
            }
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
        {loading ? (
          <p role="status">正在加载{title}…</p>
        ) : !error && !data.items.length ? (
          <div className="crm-empty">
            <h2>{query ? "没有符合条件的记录" : `还没有${title}`}</h2>
            <p>
              {query
                ? "调整搜索条件后重试。"
                : contacts
                  ? "在商机中填写联系人，即可在这里查看。"
                  : "打开商机，记录一次客户沟通。"}
            </p>
            <a href="/#crm">查看商机</a>
          </div>
        ) : (
          data.items.map((item) =>
            "opportunities" in item ? (
              <article
                className="crm-register-card"
                key={item.opportunities.map((o) => o.id).join(",")}
              >
                <h2>{item.contactName}</h2>
                <p className="crm-company">{item.companyName}</p>
                <dl className="crm-facts">
                  <div>
                    <dt>联系方式</dt>
                    <dd>{item.contactDetails || "未填写"}</dd>
                  </div>
                  <div>
                    <dt>关联商机</dt>
                    <dd className="crm-related">
                      {item.opportunities.map((o) => (
                        <a key={o.id} href={opportunityLink(o.id)}>
                          {o.title}
                        </a>
                      ))}
                    </dd>
                  </div>
                </dl>
                <p className="crm-hint">如需更新联系人，请打开关联商机编辑。</p>
              </article>
            ) : (
              <article className="crm-register-card" key={item.id}>
                <div className="crm-row-top">
                  <a href={opportunityLink(item.opportunityId)}>
                    {item.opportunityTitle}
                  </a>
                  <span className={`crm-stage crm-stage-${item.stage}`}>
                    {CRM_STAGES[item.stage]}
                  </span>
                </div>
                <p className="crm-company">
                  {item.companyName}
                  {item.contactName && ` · ${item.contactName}`}
                </p>
                <p className="crm-note-content">{item.note}</p>
                <div className="crm-note-meta">
                  <strong>{item.authorName}</strong>
                  <time dateTime={item.createdAt}>
                    {new Date(item.createdAt).toLocaleString("zh-CN")}
                  </time>
                </div>
                {(item.nextAction || item.nextFollowUp) && (
                  <p className="crm-hint">
                    下一步：{item.nextAction || "未填写"}
                    {item.nextFollowUp && ` · ${item.nextFollowUp}`}
                  </p>
                )}
              </article>
            ),
          )
        )}
      </div>
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
