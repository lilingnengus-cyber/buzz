import React from "react";
import { CrmLeadImport } from "./CrmLeadImport";
import { request } from "./api";
import { CrmSearchSelect } from "./CrmSearchSelect";
import { CrmDrawer } from "./CrmDrawer";
import { CrmLeadForm } from "./CrmLeadForm";
import { CrmLeadFollowup } from "./CrmLeadFollowup";
import { CrmForm } from "./CrmForm";
import { localDate, type CrmOption } from "./crm";
import {
  LEAD_STATUSES,
  leadLink,
  type Lead,
  type LeadDetail,
} from "./crmLeads";
import "./crm.css";
import "./crm-opportunities.css";
import "./crm-leads.css";
type LeadList = { items: Lead[]; hasMore: boolean; canManage: boolean };
export function CrmLeadsPage({ initialId }: { initialId?: string }) {
  const [importing, setImporting] = React.useState(false);
  const [owner, setOwner] = React.useState("");
  const [ownerQuery, setOwnerQuery] = React.useState("");
  const [owners, setOwners] = React.useState<{ id: string; name: string }[]>(
    [],
  );
  const [ownerError, setOwnerError] = React.useState("");
  const [data, setData] = React.useState<LeadList>({
    items: [],
    hasMore: false,
    canManage: false,
  });
  const [query, setQuery] = React.useState("");
  const [status, setStatus] = React.useState("");
  const [overdue, setOverdue] = React.useState(false);
  const [offset, setOffset] = React.useState(0);
  const [revision, setRevision] = React.useState(0);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState("");
  const [notice, setNotice] = React.useState("");
  const [selected, setSelected] = React.useState<string | null>(
    initialId ?? null,
  );
  const [mode, setMode] = React.useState("edit");
  React.useEffect(() => {
    let active = true;
    setOwnerError("");
    request<{ items: { id: string; name: string }[] }>(
      "/api/v1/crm/leads/owners",
    )
      .then((r) => {
        if (active) setOwners(r.items);
      })
      .catch((e) => {
        if (active) setOwnerError(e.message);
      });
    return () => {
      active = false;
    };
  }, [revision]);
  React.useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    const timer = setTimeout(() => {
      const params = new URLSearchParams({ offset: String(offset) });
      if (query.trim()) params.set("query", query.trim());
      if (status) params.set("status", status);
      if (owner) params.set("ownerUserId", owner);
      if (overdue) params.set("dueBy", localDate());
      request<LeadList>(`/api/v1/crm/leads?${params}`)
        .then((r) => {
          if (active) setData(r);
        })
        .catch((e) => {
          if (active) {
            setError(e.message);
            setData({ items: [], hasMore: false, canManage: false });
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
  }, [query, status, overdue, offset, revision, owner]);
  const close = () => {
    setSelected(null);
    setMode("detail");
    window.history.replaceState(null, "", "#crmLeads");
  };
  const saved = (id: string, transferred: boolean) => {
    setMode("detail");
    setSelected(transferred ? null : id);
    setRevision((v) => v + 1);
    setNotice(
      transferred
        ? "线索已交给所选负责人，可在对方的线索页面继续跟进。"
        : "线索已保存",
    );
  };
  return (
    <section className="crm-page crm-opportunities crm-leads">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>线索</h1>
          <p className="crm-hint">
            筛选需求，确认值得推进后转为商机。显示自己负责或创建的线索。
          </p>
        </div>
        {data.canManage && (
          <button
            className="primary"
            onClick={() => {
              setSelected(null);
              setMode("create");
            }}
          >
            新建线索
          </button>
        )}
        {data.canManage && !error && (
          <button onClick={() => setImporting(true)}>批量导入</button>
        )}
      </header>
      {importing && <CrmLeadImport onClose={() => setImporting(false)} onChanged={() => setRevision((v) => v + 1)} />}
      {notice && (
        <p role="status" className="crm-notice">
          {notice}
        </p>
      )}
      <div className="crm-toolbar">
        <label className="crm-search">
          搜索线索
          <input
            type="search"
            value={query}
            maxLength={160}
            placeholder="线索、公司、联系人或联系方式"
            onChange={(e) => {
              setQuery(e.target.value);
              setOffset(0);
            }}
          />
        </label>
        <button
          aria-pressed={overdue}
          onClick={() => {
            setOverdue(!overdue);
            setOffset(0);
          }}
        >
          逾期未跟进
        </button>
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
      {ownerError ? (
        <p role="alert">
          负责人选项加载失败：{ownerError}{" "}
          <button onClick={() => setRevision((v) => v + 1)}>重试负责人</button>
        </p>
      ) : (
        <CrmSearchSelect
          label="筛选负责人"
          value={owner}
          query={ownerQuery}
          onQuery={setOwnerQuery}
          options={[
            { value: "", label: "全部负责人" },
            ...owners
              .filter((o) => o.name.includes(ownerQuery))
              .map((o) => ({ value: o.id, label: o.name })),
          ]}
          onChange={(v) => {
            setOwner(v);
            setOffset(0);
          }}
        />
      )}
      <nav className="crm-stage-nav" aria-label="按线索状态筛选">
        {[["", "全部线索"], ...Object.entries(LEAD_STATUSES)].map(([k, v]) => (
          <button
            key={k}
            aria-pressed={status === k}
            onClick={() => {
              setStatus(k);
              setOffset(0);
            }}
          >
            {v}
          </button>
        ))}
      </nav>
      {error && (
        <p role="alert" className="crm-error">
          {error}{" "}
          <button onClick={() => setRevision((v) => v + 1)}>重新加载</button>
        </p>
      )}
      <div className="crm-register" aria-busy={loading}>
        <div className="crm-list-caption">
          <strong>线索筛选</strong>
          <span>点击记录查看详情与跟进</span>
        </div>
        <div className="crm-lead-columns" aria-hidden="true">
          {["线索名称", "公司名称", "联系人", "状态", "下一步", "跟进日期"].map((v) => (
            <span key={v}>{v}</span>
          ))}
        </div>
        {loading ? (
          <p className="crm-empty" role="status">
            正在加载线索…
          </p>
        ) : !error && !data.items.length ? (
          <p className="crm-empty">
            暂无符合条件的线索。新建线索，先记下一个需求。
          </p>
        ) : (
          data.items.map((item) => (
            <button
              className="crm-row crm-lead-row"
              key={item.id}
              onClick={() => {
                setSelected(item.id);
                setMode("edit");
              }}
            >
              <span data-label="线索名称">
                <strong>{item.title}</strong>
              </span>
              <span data-label="公司名称">{item.companyName || "公司待确认"}</span>
              <span data-label="联系人">{item.contactName || "未填写"}</span>
              <span data-label="状态">{LEAD_STATUSES[item.status]}</span>
              <span data-label="下一步">{item.nextAction || "未安排"}</span>
              <span data-label="跟进日期">{item.nextFollowUp || "未安排"}</span>
            </button>
          ))
        )}
      </div>
      {(offset > 0 || data.hasMore) && (
        <nav className="crm-pagination" aria-label="线索分页">
          <button
            disabled={loading || !offset}
            onClick={() => setOffset(offset - 50)}
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
      {(selected || mode === "create") && (
        <CrmDrawer
          title={mode === "create" ? "新建线索" : "线索详情"}
          onClose={close}
        >
          {mode === "create" ? (
            <CrmLeadForm onSaved={saved} onCancel={close} />
          ) : (
            selected && (
              <LeadRecord
                key={`${selected}-${revision}`}
                id={selected}
                canManage={data.canManage}
                defaultEdit={mode === "edit"}
                onSaved={saved}
              />
            )
          )}
        </CrmDrawer>
      )}
    </section>
  );
}
function LeadRecord({
  id,
  canManage,
  defaultEdit,
  onSaved,
}: {
  id: string;
  canManage: boolean;
  defaultEdit: boolean;
  onSaved: (id: string, transferred: boolean) => void;
}) {
  const [owner, setOwner] = React.useState("");
  const [ownerQuery, setOwnerQuery] = React.useState("");
  const [owners, setOwners] = React.useState<{ id: string; name: string }[]>(
    [],
  );
  const [ownerError, setOwnerError] = React.useState("");
  const [data, setData] = React.useState<LeadDetail | null>(null);
  const [error, setError] = React.useState("");
  const [revision, setRevision] = React.useState(0);
  const [mode, setMode] = React.useState(defaultEdit ? "initial" : "detail");
  const [offset, setOffset] = React.useState(0);
  const [options, setOptions] = React.useState<CrmOption[] | null>(null);
  const [optionsError, setOptionsError] = React.useState("");
  React.useEffect(() => {
    let active = true;
    setError("");
    setData(null);
    request<LeadDetail>(`/api/v1/crm/leads/${id}?offset=${offset}`)
      .then((r) => {
        if (active) setData(r);
      })
      .catch((e) => {
        if (active) setError(e.message);
      });
    return () => {
      active = false;
    };
  }, [id, revision, offset]);
  React.useEffect(() => {
    if (mode !== "convert") return;
    let active = true;
    setOptions(null);
    setOptionsError("");
    request<{ items: CrmOption[] }>("/api/v1/crm/options")
      .then((r) => {
        if (active) setOptions(r.items);
      })
      .catch((e) => {
        if (active) setOptionsError(e.message);
      });
    return () => {
      active = false;
    };
  }, [mode, revision]);
  if (error)
    return (
      <p role="alert">
        {error}{" "}
        <button onClick={() => setRevision((v) => v + 1)}>重试详情</button>
      </p>
    );
  if (!data) return <p role="status">正在加载详情…</p>;
  const item = data.item;
  if ((mode === "edit" || mode === "initial") && canManage && item.status !== "converted")
    return (
      <CrmLeadForm
        record={item}
        onSaved={onSaved}
        onCancel={() => setMode("detail")}
      />
    );
  if (mode === "followup")
    return (
      <CrmLeadFollowup
        item={item}
        onSaved={() => {
          setMode("detail");
          onSaved(id, false);
        }}
        onCancel={() => setMode("detail")}
      />
    );
  if (mode === "convert")
    return (
      <>
        {optionsError ? (
          <p role="alert">
            {optionsError}{" "}
            <button onClick={() => setRevision((v) => v + 1)}>重试选项</button>
          </p>
        ) : options ? (
          <CrmForm
            lead={item}
            options={options}
            onSaved={(newId) =>
              window.location.assign(
                `/#crm?opportunity=${encodeURIComponent(newId)}`,
              )
            }
            onCancel={() => setMode("detail")}
          />
        ) : (
          <p role="status">正在加载商机选项…</p>
        )}
      </>
    );
  return (
    <div className="crm-record-detail">
      <header className="crm-heading">
        <div>
          <span className="crm-stage">{LEAD_STATUSES[item.status]}</span>
          <p className="crm-hint">线索名称</p>
          <h2>{item.title}</h2>
        </div>
        {canManage && item.status !== "converted" && (
          <button onClick={() => setMode("edit")}>编辑线索</button>
        )}
      </header>
      {data.duplicates.length > 0 && (
        <aside className="crm-notice">
          发现公司或联系方式相同的线索，请核对：
          {data.duplicates.map((d) => (
            <p key={d.id}>
              <a href={leadLink(d.id)}>{d.title}</a>
            </p>
          ))}
        </aside>
      )}
      <dl className="crm-facts">
        {[
          ["公司名称", item.companyName],
          ["联系人", item.contactName],
          ["联系方式", item.contactDetails],
          ["来源", item.source],
          ["负责人", item.ownerName],
          ["下一步", item.nextAction],
          ["跟进日期", item.nextFollowUp],
        ].map(([k, v]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{v || "未填写"}</dd>
          </div>
        ))}
      </dl>
      <section>
        <h3>需求摘要</h3>
        <p className="crm-note-content">{item.summary || "未填写"}</p>
      </section>
      {item.status === "disqualified" && (
        <p>淘汰原因：{item.disqualificationReason}</p>
      )}
      {item.convertedOpportunityId ? (
        <a
          href={`/#crm?opportunity=${encodeURIComponent(item.convertedOpportunityId)}`}
        >
          打开已转入的商机
        </a>
      ) : (
        canManage && (
          <div className="crm-heading">
            <button onClick={() => setMode("followup")}>
              {item.status === "disqualified" ? "重新跟进" : "记录跟进 / 淘汰"}
            </button>
            {item.status !== "disqualified" && (
              <button className="primary" onClick={() => setMode("convert")}>
                转为商机
              </button>
            )}
          </div>
        )
      )}
      <section>
        <h3>筛选与跟进历史</h3>
        {!data.followups.length && <p className="crm-hint">暂无跟进记录</p>}
        {data.followups.map((n) => (
          <article className="crm-next" key={n.id}>
            <p className="crm-hint">
              {n.authorName} · {new Date(n.createdAt).toLocaleString("zh-CN")} ·{" "}
              {LEAD_STATUSES[n.status]}
            </p>
            <p className="crm-note-content">{n.note}</p>
            {n.disqualificationReason && (
              <p>淘汰原因：{n.disqualificationReason}</p>
            )}
            <p>下一步：{n.nextAction || "未安排"}</p>
            <p>跟进日期：{n.nextFollowUp || "未安排"}</p>
          </article>
        ))}
      </section>
      {(offset > 0 || data.hasMore) && (
        <nav className="crm-pagination">
          <button disabled={!offset} onClick={() => setOffset(offset - 100)}>
            较新记录
          </button>
          <button
            disabled={!data.hasMore}
            onClick={() => setOffset(offset + 100)}
          >
            较早记录
          </button>
        </nav>
      )}
    </div>
  );
}
