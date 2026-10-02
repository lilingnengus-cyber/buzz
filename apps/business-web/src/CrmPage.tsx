import React from "react";
import { request } from "./api";
import { formatMoney } from "./formatters";
import { CrmForm } from "./CrmForm";
import { CrmDetail } from "./CrmDetail";
import {
  CRM_STAGES,
  isDue,
  localDate,
  type CrmDetail as Detail,
  type CrmOption,
  type Opportunity,
} from "./crm";
import "./crm.css";
import "./crm-opportunities.css";
function CrmDrawer({
  children,
  title,
  onClose,
}: {
  children: React.ReactNode;
  title: string;
  onClose: () => void;
}) {
  const ref = React.useRef<HTMLDialogElement>(null);
  React.useEffect(() => {
    const dialog = ref.current;
    const previousOverflow = document.body.style.overflow;
    dialog?.showModal();
    document.body.style.overflow = "hidden";
    return () => {
      dialog?.close();
      document.body.style.overflow = previousOverflow;
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className="crm-drawer"
      aria-label={title}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="crm-drawer-content">
        <header className="crm-drawer-heading">
          <strong>{title}</strong>
          <button aria-label="关闭商机弹窗" onClick={onClose}>
            关闭 ×
          </button>
        </header>
        {children}
      </div>
    </dialog>
  );
}
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
  const [due, setDue] = React.useState(false);
  const [offset, setOffset] = React.useState(0);
  const [selected, setSelected] = React.useState<string | null>(
    initialId ?? null,
  );
  const [detail, setDetail] = React.useState<Detail | null>(null);
  const [editing, setEditing] = React.useState(false);
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
      if (due) params.set("dueBy", localDate());
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
  }, [query, stage, due, offset, revision]);
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
          <p className="crm-hint">从线索到成交，让每个商机都有下一步。</p>
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
      </header>
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
        <button
          aria-pressed={due}
          onClick={() => {
            setDue(!due);
            setOffset(0);
          }}
        >
          待跟进（今天及逾期）
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
            <span>下一步</span>
            <span>跟进日期</span>
          </div>
          {loading ? (
            <p className="crm-empty" role="status">
              正在加载商机…
            </p>
          ) : !error && !data.items.length ? (
            <div className="crm-empty">
              <h2>
                {query || stage || due
                  ? "没有符合条件的商机"
                  : "从一个潜在客户开始"}
              </h2>
              <p>
                {query || stage || due
                  ? "调整筛选条件，或新建商机。"
                  : "新建商机，记下需求和下一步跟进。"}
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
                <span className="crm-opportunity-next" data-label="下一步">
                  {item.nextAction || "待安排下一步"}
                </span>
                <span
                  data-label="跟进日期"
                  className={`crm-opportunity-date ${isDue(item) ? "crm-due" : "crm-followup-date"}`}
                >
                  {item.stage === "won" || item.stage === "lost"
                    ? "已结束"
                    : item.nextFollowUp
                      ? `${item.nextFollowUp === localDate() ? "今天" : item.nextFollowUp} 跟进${isDue(item) && item.nextFollowUp < localDate() ? " · 已逾期" : ""}`
                      : "未安排跟进日期"}
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
                  onRefresh={refresh}
                />
              ))
            )}
          </CrmDrawer>
        )}
      </div>
    </section>
  );
}
