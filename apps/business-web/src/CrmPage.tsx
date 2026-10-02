import React from "react";
import { CrmImport } from "./CrmImport";
import { request } from "./api";
import { formatMoney } from "./formatters";
import { CrmForm } from "./CrmForm";
import { CrmDrawer } from "./CrmDrawer";
import { CrmDetail } from "./CrmDetail";
import {
  CRM_STAGES,
  localDate,
  type CrmDetail as Detail,
  type CrmOption,
  type Opportunity,
} from "./crm";
import "./crm.css";
import "./crm-opportunities.css";
type List = { items: Opportunity[]; hasMore: boolean; canManage: boolean };
export function CrmPage({ initialId }: { initialId?: string }) {
  const [data, setData] = React.useState<List>({
    items: [],
    hasMore: false,
    canManage: false,
  });
  const [options, setOptions] = React.useState<CrmOption[]>([]);
  const [query, setQuery] = React.useState("");
  const [stage, setStage] = React.useState("");
  const [mine, setMine] = React.useState(false);
  const [due, setDue] = React.useState("open");
  const [offset, setOffset] = React.useState(0);
  const [selected, setSelected] = React.useState<string | null>(
    initialId ?? null,
  );
  const [detail, setDetail] = React.useState<Detail | null>(null);
  const [editing, setEditing] = React.useState(false);
  const [importing, setImporting] = React.useState(false);
  const [creating, setCreating] = React.useState(false);
  const [loading, setLoading] = React.useState(true);
  const [detailLoading, setDetailLoading] = React.useState(false);
  const [error, setError] = React.useState("");
  const [detailError, setDetailError] = React.useState("");
  const [revision, setRevision] = React.useState(0);
  const [notice, setNotice] = React.useState("");
  React.useEffect(() => {
    let current = true;
    setLoading(true);
    setError("");
    const timer = setTimeout(() => {
      const params = new URLSearchParams({ offset: String(offset) });
      if (query.trim()) params.set("query", query.trim());
      if (stage) params.set("stage", stage);
      if (mine) params.set("mine", "true");
      if (due) {
        params.set("followup", due);
        params.set("today", localDate());
      }
      Promise.all([
        request<List>(`/api/v1/crm/opportunities?${params}`),
        request<{ items: CrmOption[] }>("/api/v1/crm/options"),
      ])
        .then(([list, choices]) => {
          if (current) {
            setData(list);
            setOptions(choices.items);
          }
        })
        .catch((e) => {
          if (current) {
            setData({ items: [], hasMore: false, canManage: false });
            setError(e instanceof Error ? e.message : "商机加载失败");
          }
        })
        .finally(() => {
          if (current) setLoading(false);
        });
    }, 200);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [query, stage, due, mine, offset, revision]);
  React.useEffect(() => {
    let current = true;
    setDetail(null);
    setDetailError("");
    if (!selected) return;
    setDetailLoading(true);
    request<Detail>(`/api/v1/crm/opportunities/${selected}`)
      .then((d) => {
        if (current) setDetail(d);
      })
      .catch((e) => {
        if (current)
          setDetailError(e instanceof Error ? e.message : "商机详情加载失败");
      })
      .finally(() => {
        if (current) setDetailLoading(false);
      });
    return () => {
      current = false;
    };
  }, [selected, revision]);
  const saved = (id: string) => {
    setCreating(false);
    setEditing(false);
    setSelected(id);
    setRevision((v) => v + 1);
    setNotice("商机已保存");
  };
  const refresh = async () => {
    setRevision((v) => v + 1);
    setNotice("跟进已保存");
  };
  return (
    <section className="crm-page crm-opportunities">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>商机</h1>
          <p className="crm-hint">集中管理客户需求与销售进展。</p>
        </div>
        {data.canManage && !creating && (
          <button
            className="primary"
            onClick={() => {
              setCreating(true);
              setEditing(false);
              setSelected(null);
              setNotice("");
            }}
          >
            新建商机
          </button>
        )}
        {data.canManage && (
          <button onClick={() => setImporting(true)}>批量导入</button>
        )}
      </header>
      {importing && (
        <CrmImport
          options={options}
          onClose={() => setImporting(false)}
          onChanged={() => setRevision((v) => v + 1)}
        />
      )}
      {notice && (
        <p role="status" className="crm-notice">
          {notice}
        </p>
      )}
      <div className="crm-toolbar">
        <label className="crm-search">
          搜索商机
          <input
            type="search"
            value={query}
            maxLength={160}
            placeholder="公司、商机或联系人"
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
        <button
          aria-pressed={mine}
          onClick={() => {
            setMine(!mine);
            setOffset(0);
          }}
        >
          我的商机
        </button>
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
      <nav className="crm-stage-nav" aria-label="按销售阶段筛选">
        {[["", "全部商机"], ...Object.entries(CRM_STAGES)].map(
          ([value, label]) => (
            <button
              key={value}
              aria-pressed={stage === value}
              onClick={() => {
                setStage(value);
                if (value === "won" || value === "lost") setDue("");
                setOffset(0);
              }}
            >
              {label}
            </button>
          ),
        )}
      </nav>
      {error && (
        <p role="alert" className="crm-error">
          {error}{" "}
          <button onClick={() => setRevision((v) => v + 1)}>重新加载</button>
        </p>
      )}
      <div className="crm-layout">
        <div className="crm-register" aria-busy={loading}>
          <div className="crm-list-caption">
            <strong>
              {stage
                ? CRM_STAGES[stage as keyof typeof CRM_STAGES]
                : "全部商机"}
            </strong>
            <span>
              {loading ? "正在更新" : `本页 ${data.items.length} 个商机`} ·
              点击商机查看与跟进
            </span>
          </div>
          <div className="crm-list-columns" aria-hidden="true">
            <span>商机</span>
            <span>客户</span>
            <span>联系人</span>
            <span>销售阶段</span>
            <span>预计金额</span>
          </div>
          {loading ? (
            <p className="crm-empty" role="status">
              正在加载商机…
            </p>
          ) : !error && !data.items.length ? (
            <div className="crm-empty">
              <h2>
                {query || stage || due || mine
                  ? "没有符合条件的商机"
                  : "从一个潜在客户开始"}
              </h2>
              <p>
                {query || stage || due || mine
                  ? "调整筛选条件，或新建商机。"
                  : "新建商机，记录客户需求。"}
              </p>
            </div>
          ) : (
            data.items.map((item) => (
              <button
                key={item.id}
                className={`crm-row${selected === item.id ? " selected" : ""}`}
                aria-pressed={selected === item.id}
                onClick={() => {
                  setSelected(item.id);
                  setCreating(false);
                  setEditing(false);
                  setNotice("");
                }}
              >
                <span className="crm-opportunity-identity">
                  <strong>{item.title}</strong>
                  {item.ownerName && (
                    <span className="crm-contact">
                      负责人：{item.ownerName}
                    </span>
                  )}
                </span>
                <span className="crm-company" data-label="客户">
                  {item.companyName}
                </span>
                <span className="crm-contact" data-label="联系人">
                  {item.contactName || "待补充"}
                </span>
                <span className={`crm-stage crm-stage-${item.stage}`}>
                  {CRM_STAGES[item.stage]}
                </span>
                <span className="crm-opportunity-amount">
                  {item.expectedAmountMinor == null
                    ? "金额待确认"
                    : formatMoney(
                        item.currency,
                        item.expectedAmountMinor / 100,
                      )}
                </span>
              </button>
            ))
          )}
          {(offset > 0 || data.hasMore) && (
            <nav className="crm-pagination" aria-label="商机分页">
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
        </div>
        {(selected || creating) && (
          <CrmDrawer
            title={creating ? "新建商机" : "商机详情"}
            onClose={() => {
              setSelected(null);
              setCreating(false);
              setEditing(false);
            }}
          >
            {creating ? (
              <CrmForm
                options={options}
                onSaved={saved}
                onCancel={() => setCreating(false)}
              />
            ) : detailLoading ? (
              <p role="status">正在加载详情…</p>
            ) : detailError ? (
              <p role="alert" className="crm-error">
                {detailError}
                <button onClick={() => setRevision((v) => v + 1)}>
                  重新加载
                </button>
              </p>
            ) : (
              detail &&
              (editing ? (
                <CrmForm
                  key={`${detail.item.id}-${detail.item.version}`}
                  record={detail.item}
                  options={options}
                  onSaved={saved}
                  onCancel={() => setEditing(false)}
                />
              ) : (
                <CrmDetail
                  key={`${detail.item.id}-${detail.item.version}`}
                  data={detail}
                  canManage={data.canManage}
                  onEdit={() => setEditing(true)}
                />
              ))
            )}
          </CrmDrawer>
        )}
      </div>
    </section>
  );
}
