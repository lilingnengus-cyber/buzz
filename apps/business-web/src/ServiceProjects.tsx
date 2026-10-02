import { formatAmount } from "./formatters";
import React from "react";
import { request } from "./api";
import { CrmDrawer, useCrmDraft } from "./CrmDrawer";
import { useCrmCommand } from "./useCrmCommand";
import { OperatingUnitPicker } from "./OperatingUnitPicker";
import "./crm.css";
import "./service-projects.css";
import "./crm-opportunities.css";

type RecordData = {
  id: string;
  version: number;
  title: string;
  status: string;
  project_id?: string;
  project_title?: string;
  customer_name?: string;
  owner_name?: string;
  legal_entity_id: string;
  business_unit_id: string;
  customer_id: string;
  owner_user_id: string;
  contact_name?: string;
  service_kind?: string;
  starts_on?: string;
  ends_on?: string;
  due_on?: string;
  description?: string;
  evidence_url?: string;
  sales_order_line_id?: string;
  renewal_of_project_id?: string;
  sales_order_id?: string;
  order_number?: string;
};
type Choice = {
  id: string;
  name: string;
  code: string;
  status: string;
  resourceType: string;
  parentBusinessUnitId?: string;
};
type Choices = {
  items: Choice[];
  owners: { id: string; name: string }[];
  currentUserId: string;
  orderLines?: (RecordData & { amount: string; currency: string })[];
  hasMoreOrders?: boolean;
};
type Detail = {
  canAccept?: boolean;
  receivable?: {
    number: string;
    amount: string;
    openAmount: string;
    currency: string;
    dueDate: string;
  };
  item: RecordData;
  tasks: RecordData[];
  acceptances: {
    id: string;
    accepted_on: string;
    customer_reviewer: string;
    result: string;
    note: string;
    evidence_url: string;
  }[];
  hasMoreTasks: boolean;
  hasMoreAcceptances: boolean;
};
const statuses: Record<string, string> = {
  pending: "待启动",
  active: "进行中",
  acceptance: "待验收",
  completed: "已完成",
  paused: "已暂停",
  cancelled: "已取消",
};
const localDay = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
export function ServiceProjects({ tasks = false }: { tasks?: boolean }) {
  const [rows, setRows] = React.useState<RecordData[]>([]),
    [query, setQuery] = React.useState(""),
    [status, setStatus] = React.useState(""),
    [expiry, setExpiry] = React.useState(""),
    [offset, setOffset] = React.useState(0),
    [hasMore, setHasMore] = React.useState(false),
    [canManage, setCanManage] = React.useState(false),
    [error, setError] = React.useState(""),
    [busy, setBusy] = React.useState(false),
    [revision, setRevision] = React.useState(0),
    [selected, setSelected] = React.useState<string | null>(null),
    [create, setCreate] = React.useState(
      () =>
        !tasks &&
        new URLSearchParams(window.location.hash.split("?")[1]).has("order"),
    );
  React.useEffect(() => {
    let active = true;
    setBusy(true);
    setError("");
    const q = new URLSearchParams({ offset: String(offset) });
    if (query) q.set("query", query);
    if (status) q.set("status", status);
    if (expiry) {
      q.set("expiry", expiry);
      q.set("today", localDay());
    }
    request<{ items: RecordData[]; hasMore: boolean; canManage: boolean }>(
      `/api/v1/${tasks ? "service-deliverables" : "service-projects"}?${q}`,
    )
      .then((d) => {
        if (active) {
          setRows(d.items);
          setHasMore(d.hasMore);
          setCanManage(d.canManage);
        }
      })
      .catch((e) => active && setError(e.message))
      .finally(() => active && setBusy(false));
    return () => {
      active = false;
    };
  }, [tasks, query, status, expiry, offset, revision]);
  const title = tasks ? "交付事项" : "服务项目";
  return (
    <section className="crm-page service-projects">
      <header className="crm-heading">
        <div>
          <p>业务闭环</p>
          <h1>{title}</h1>
          <p>
            {tasks
              ? "按项目记录交付内容与进度。"
              : "技术服务与软件服务的交付、验收和服务期限。"}
          </p>
        </div>
        {canManage && !tasks && (
          <button className="primary" onClick={() => setCreate(true)}>
            新建服务项目
          </button>
        )}
      </header>
      <div className="crm-filters">
        <label>
          搜索
          <input
            type="search"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setOffset(0);
            }}
          />
        </label>
        <label>
          状态
          <select
            value={status}
            onChange={(e) => {
              setStatus(e.target.value);
              setOffset(0);
            }}
          >
            <option value="">全部</option>
            {Object.entries(statuses)
              .filter(([v]) => !tasks || !["acceptance", "paused"].includes(v))
              .map(([v, n]) => (
                <option key={v} value={v}>
                  {n}
                </option>
              ))}
          </select>
        </label>
        <label>
          {tasks ? "计划日期" : "软件服务到期"}
          <select
            value={expiry}
            onChange={(e) => {
              setExpiry(e.target.value);
              setOffset(0);
            }}
          >
            <option value="">全部</option>
            <option value="expired">{tasks ? "已逾期" : "已到期"}</option>
            <option value="upcoming">未来30天</option>
          </select>
        </label>
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
      {error ? (
        <p role="alert">{error}</p>
      ) : busy ? (
        <p>正在读取…</p>
      ) : (
        <div className="crm-table">
          {rows.length === 0 && <p>暂无记录</p>}
          {rows.map((r) => (
            <button
              className="crm-opportunity-row"
              key={r.id}
              onClick={() => setSelected(tasks ? (r.project_id ?? null) : r.id)}
            >
              <strong>{r.title}</strong>
              <span>{tasks ? r.project_title : r.customer_name}</span>
              <span>{statuses[r.status]}</span>
              <span>{tasks ? r.due_on : r.ends_on}</span>
            </button>
          ))}
        </div>
      )}
      <div className="crm-pagination">
        <button
          disabled={offset === 0 || busy}
          onClick={() => setOffset((v) => Math.max(0, v - 50))}
        >
          上一页
        </button>
        <span>第{offset / 50 + 1}页</span>
        <button
          disabled={!hasMore || busy}
          onClick={() => setOffset((v) => v + 50)}
        >
          下一页
        </button>
      </div>
      {create && (
        <CrmDrawer title="新建服务项目" onClose={() => setCreate(false)}>
          <ServiceForm
            onSaved={() => {
              setCreate(false);
              setRevision((v) => v + 1);
            }}
          />
        </CrmDrawer>
      )}
      {selected && (
        <CrmDrawer title="服务项目详情" onClose={() => setSelected(null)}>
          <ProjectDetail
            id={selected}
            canManage={canManage}
            onSaved={() => setRevision((v) => v + 1)}
          />
        </CrmDrawer>
      )}
    </section>
  );
}
function ProjectDetail({
  id,
  canManage,
  onSaved,
}: {
  id: string;
  canManage: boolean;
  onSaved: () => void;
}) {
  const [data, setData] = React.useState<Detail>(),
    [error, setError] = React.useState(""),
    [revision, setRevision] = React.useState(0),
    [mode, setMode] = React.useState<"view" | "project" | "task" | "accept">(
      "view",
    ),
    [task, setTask] = React.useState<RecordData>();
  const draft = useCrmDraft();
  React.useEffect(() => {
    let active = true;
    setError("");
    request<Detail>(`/api/v1/service-projects/${id}`)
      .then((d) => active && setData(d))
      .catch((e) => active && setError(e.message));
    return () => {
      active = false;
    };
  }, [id, revision]);
  const saved = () => {
    draft.saved();
    setMode("view");
    setRevision((v) => v + 1);
    onSaved();
  };
  if (error)
    return (
      <p role="alert">
        {error}
        <button onClick={() => setRevision((v) => v + 1)}>重试</button>
      </p>
    );
  if (!data) return <p>正在读取…</p>;
  const p = data.item,
    closed = ["completed", "cancelled"].includes(p.status);
  if (mode !== "view")
    return (
      <>
        <button onClick={() => draft.discard(() => setMode("view"))}>
          返回详情
        </button>
        {mode === "accept" ? (
          <AcceptanceForm
            project={p}
            canAccept={!!data.canAccept}
            onSaved={saved}
          />
        ) : (
          <ServiceForm
            item={mode === "project" ? p : task}
            project={mode === "task" ? p : undefined}
            onSaved={saved}
          />
        )}
      </>
    );
  return (
    <>
      <h2>{p.title}</h2>
      <p>
        {p.customer_name} · {statuses[p.status]} ·{" "}
        {p.service_kind === "software_service" ? "软件服务" : "技术服务"}
      </p>
      <p>
        负责人：{p.owner_name}　联系人：{p.contact_name || "未填写"}
      </p>
      <p>
        服务期限：{p.starts_on || "未设置"} — {p.ends_on || "未设置"}
      </p>
      <p>{p.description}</p>
      {p.sales_order_id && (
        <a href={`/sales/orders/${p.sales_order_id}`}>
          查看订单与回款：{p.order_number}
        </a>
      )}
      {data.receivable && (
        <p role="status">
          应收 {data.receivable.number} · {data.receivable.currency}{" "}
          {formatAmount(data.receivable.amount)} · 未收 {formatAmount(data.receivable.openAmount)} · 到期{" "}
          {data.receivable.dueDate}
        </p>
      )}
      {canManage && !closed && (
        <div>
          <button onClick={() => setMode("project")}>编辑项目</button>
          <button
            onClick={() => {
              setTask(undefined);
              setMode("task");
            }}
          >
            新建交付事项
          </button>
          {p.status === "acceptance" && (
            <button onClick={() => setMode("accept")}>记录验收结果</button>
          )}
        </div>
      )}
      <h3>交付事项</h3>
      {data.tasks.map((t) => (
        <div key={t.id}>
          <strong>{t.title}</strong>
          <p>
            {statuses[t.status]} · {t.due_on || "未设置日期"}
          </p>
          <p>{t.description}</p>
          {t.evidence_url && (
            <a href={t.evidence_url} target="_blank" rel="noreferrer">
              交付凭据
            </a>
          )}
          {data.receivable && (
            <p role="status">
              应收 {data.receivable.number} · {data.receivable.currency}{" "}
              {formatAmount(data.receivable.amount)} · 未收 {formatAmount(data.receivable.openAmount)} ·
              到期 {data.receivable.dueDate}
            </p>
          )}
          {canManage && !closed && (
            <button
              onClick={() => {
                setTask(t);
                setMode("task");
              }}
            >
              编辑事项
            </button>
          )}
        </div>
      ))}
      {data.hasMoreTasks && <p>仅显示前500项，请在交付事项页面查询。</p>}
      <h3>验收记录</h3>
      {data.acceptances.map((a) => (
        <div key={a.id}>
          <p>
            {a.accepted_on} · {a.customer_reviewer} ·{" "}
            {a.result === "passed" ? "通过" : "未通过"}
          </p>
          <p>{a.note}</p>
          {a.evidence_url && (
            <a href={a.evidence_url} target="_blank" rel="noreferrer">
              验收凭据
            </a>
          )}
        </div>
      ))}
      {data.hasMoreAcceptances && <p>仅显示最近100条验收记录。</p>}
    </>
  );
}
function ServiceForm({
  item,
  project,
  onSaved,
}: {
  item?: RecordData;
  project?: RecordData;
  onSaved: () => void;
}) {
  const draft = useCrmDraft(),
    command = useCrmCommand();
  const [options, setOptions] = React.useState<Choices>(),
    [error, setError] = React.useState(""),
    [busy, setBusy] = React.useState(false);
  const lock = React.useRef(false);
  const [sourceQuery, setSourceQuery] = React.useState(
    () =>
      new URLSearchParams(window.location.hash.split("?")[1]).get("order") ||
      "",
  );
  const [form, setForm] = React.useState({
    title: item?.title ?? "",
    legalEntityId: item?.legal_entity_id ?? "",
    businessUnitId: item?.business_unit_id ?? "",
    customerId: item?.customer_id ?? "",
    ownerUserId: item?.owner_user_id ?? "",
    contactName: item?.contact_name ?? "",
    serviceKind: item?.service_kind ?? "technical_service",
    startsOn: item?.starts_on ?? "",
    endsOn: item?.ends_on ?? "",
    dueOn: item?.due_on ?? "",
    status: item?.status ?? "pending",
    description: item?.description ?? "",
    evidenceUrl: item?.evidence_url ?? "",
    salesOrderLineId: item?.sales_order_line_id ?? "",
  });
  React.useEffect(() => {
    let active = true;
    request<Choices>(
      `/api/v1/service-project-options?query=${encodeURIComponent(sourceQuery)}`,
    )
      .then((d) => {
        if (active) {
          setOptions(d);
          setForm((f) => ({
            ...f,
            ownerUserId: f.ownerUserId || d.currentUserId,
          }));
        }
      })
      .catch((e) => active && setError(e.message));
    return () => {
      active = false;
    };
  }, [sourceQuery]);
  const set = (k: keyof typeof form, v: string) => {
    draft.markDirty();
    setForm((f) => ({ ...f, [k]: v }));
  };
  const field = (
    name: keyof typeof form,
    label: string,
    type = "text",
    required = false,
  ) => (
    <label>
      {label}
      <input
        type={type}
        value={form[name]}
        required={required}
        onChange={(e) => set(name, e.target.value)}
        maxLength={200}
      />
    </label>
  );
  const choices = (
    name: "legalEntityId" | "customerId",
    label: string,
    kind: string,
  ) => (
    <label>
      {label}
      <select
        required
        disabled={!!item || !!form.salesOrderLineId}
        value={form[name]}
        onChange={(e) => set(name, e.target.value)}
      >
        <option value="">请选择</option>
        {options?.items
          .filter((v) => v.resourceType === kind)
          .map((v) => (
            <option key={v.id} value={v.id}>
              {v.name}
            </option>
          ))}
      </select>
    </label>
  );
  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    draft.setBusy(true);
    setError("");
    try {
      const input = project
        ? {
            title: form.title,
            ownerUserId: form.ownerUserId,
            dueOn: form.dueOn || null,
            status: form.status,
            description: form.description,
            evidenceUrl: form.evidenceUrl,
            expectedVersion: item?.version ?? null,
          }
        : {
            title: form.title,
            legalEntityId: form.legalEntityId,
            businessUnitId: form.businessUnitId,
            customerId: form.customerId,
            ownerUserId: form.ownerUserId,
            contactName: form.contactName,
            serviceKind: form.serviceKind,
            startsOn: form.startsOn || null,
            endsOn: form.endsOn || null,
            status: form.status,
            description: form.description,
            salesOrderLineId: form.salesOrderLineId || null,
            renewalOfProjectId: item?.renewal_of_project_id ?? null,
            expectedVersion: item?.version ?? null,
          };
      await command(
        `/api/v1/service-projects${project ? `/${project.id}/deliverables` : ""}${item ? `/${item.id}` : ""}`,
        { method: item ? "PUT" : "POST", body: JSON.stringify(input) },
      );
      draft.saved();
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      lock.current = false;
      setBusy(false);
      draft.setBusy(false);
    }
  }
  return (
    <form className="crm-form service-form" onSubmit={save}>
      <h3>{project ? "交付事项" : "服务项目"}</h3>
      {error && <p role="alert">{error}</p>}
      <fieldset disabled={busy || !options}>
        {field("title", "名称", "text", true)}
        {!project && (
          <>
            <label>
              关联已确认服务订单
              <input
                type="search"
                placeholder="按订单号、客户或服务名称搜索"
                aria-label="搜索服务订单"
                disabled={!!item?.sales_order_line_id}
                value={sourceQuery}
                onChange={(e) => setSourceQuery(e.target.value)}
              />
              <select
                aria-label="关联已确认服务订单"
                value={form.salesOrderLineId}
                disabled={!!item?.sales_order_line_id}
                onChange={(e) => {
                  const line = options?.orderLines?.find(
                    (v) => v.id === e.target.value,
                  );
                  draft.markDirty();
                  setForm((f) => ({
                    ...f,
                    salesOrderLineId: e.target.value,
                    ...(line && !item
                      ? {
                          title: f.title || line.title,
                          legalEntityId: line.legal_entity_id,
                          businessUnitId: line.business_unit_id,
                          customerId: line.customer_id,
                          serviceKind: line.service_kind || f.serviceKind,
                        }
                      : {}),
                  }));
                }}
              >
                <option value="">暂不关联（验收通过前须关联）</option>
                {item?.sales_order_line_id && (
                  <option value={item.sales_order_line_id}>
                    {item.order_number || "已关联订单"}
                  </option>
                )}
                {options?.orderLines
                  ?.filter(
                    (v) =>
                      !item ||
                      (v.legal_entity_id === item.legal_entity_id &&
                        v.business_unit_id === item.business_unit_id &&
                        v.customer_id === item.customer_id &&
                        v.service_kind === item.service_kind),
                  )
                  .map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.order_number} · {v.title} · {v.customer_name} ·{" "}
                      {v.currency} {v.amount}
                    </option>
                  ))}
              </select>
              {options?.hasMoreOrders && (
                <small>仅显示前100项，请输入更具体的搜索词。</small>
              )}
            </label>
            {choices("legalEntityId", "法定主体", "legal_entity")}
            {choices("customerId", "客户", "customer")}
            <OperatingUnitPicker
              label="经营单元"
              records={(
                options?.items.filter(
                  (v) => v.resourceType === "business_unit",
                ) ?? []
              ).map((v) => ({
                ...v,
                parentBusinessUnitId: v.parentBusinessUnitId ?? null,
                ancestorPath: null,
                depth: null,
                descendantCount: null,
              }))}
              value={form.businessUnitId}
              onChange={(v) => set("businessUnitId", v)}
              disabled={!!item || !!form.salesOrderLineId}
            />
            {field("contactName", "联系人")}
            <label>
              服务类型
              <select
                value={form.serviceKind}
                disabled={!!item || !!form.salesOrderLineId}
                onChange={(e) => set("serviceKind", e.target.value)}
              >
                <option value="technical_service">技术服务</option>
                <option value="software_service">软件服务</option>
              </select>
            </label>
            {field("startsOn", "开始日期", "date")}
            {field("endsOn", "结束日期", "date")}
          </>
        )}
        <label>
          负责人
          <select
            required
            value={form.ownerUserId}
            onChange={(e) => set("ownerUserId", e.target.value)}
          >
            <option value="">请选择</option>
            {options?.owners.map((u) => (
              <option key={u.id} value={u.id}>
                {u.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          状态
          <select
            value={form.status}
            onChange={(e) => set("status", e.target.value)}
          >
            {Object.entries(statuses)
              .filter(([v]) =>
                project
                  ? !["acceptance", "paused"].includes(v)
                  : v !== "completed",
              )
              .map(([v, n]) => (
                <option key={v} value={v}>
                  {n}
                </option>
              ))}
          </select>
        </label>
        {project && (
          <>
            {field("dueOn", "计划完成日期", "date")}
            {field("evidenceUrl", "交付凭据链接", "url")}
          </>
        )}
        <label>
          说明
          <textarea
            maxLength={4000}
            value={form.description}
            onChange={(e) => set("description", e.target.value)}
          />
        </label>
        <button type="submit" className="primary">
          {busy ? "保存中…" : "保存"}
        </button>
      </fieldset>
    </form>
  );
}
function AcceptanceForm({
  project,
  canAccept,
  onSaved,
}: {
  project: RecordData;
  canAccept: boolean;
  onSaved: () => void;
}) {
  const draft = useCrmDraft(),
    command = useCrmCommand(),
    lock = React.useRef(false);
  const [form, setForm] = React.useState({
      acceptedOn: localDay(),
      customerReviewer: "",
      result: canAccept && project.sales_order_line_id ? "passed" : "rejected",
      note: "",
      evidenceUrl: "",
    }),
    [error, setError] = React.useState(""),
    [busy, setBusy] = React.useState(false);
  const set = (k: keyof typeof form, v: string) => {
    draft.markDirty();
    setForm((f) => ({ ...f, [k]: v }));
  };
  return (
    <form
      className="crm-form service-form"
      onSubmit={async (e) => {
        e.preventDefault();
        if (lock.current) return;
        lock.current = true;
        setBusy(true);
        draft.setBusy(true);
        setError("");
        try {
          await command(`/api/v1/service-projects/${project.id}/acceptances`, {
            method: "POST",
            body: JSON.stringify({ ...form, expectedVersion: project.version }),
          });
          draft.saved();
          onSaved();
        } catch (e) {
          setError(e instanceof Error ? e.message : "保存失败");
        } finally {
          lock.current = false;
          setBusy(false);
          draft.setBusy(false);
        }
      }}
    >
      <h3>记录验收结果</h3>
      <p>
        验收通过将按关联订单服务行金额生成应收，并确认收入；到期日按验收日期加订单账期计算。收款另行登记。
      </p>
      {!project.sales_order_line_id && (
        <p>请先编辑项目，关联已确认的服务销售订单。</p>
      )}
      {!canAccept && <p>当前账号没有验收记账权限，可记录未通过结果。</p>}
      {error && <p role="alert">{error}</p>}
      <fieldset disabled={busy}>
        <label>
          验收日期
          <input
            type="date"
            required
            value={form.acceptedOn}
            onChange={(e) => set("acceptedOn", e.target.value)}
          />
        </label>
        <label>
          客户验收人
          <input
            required
            maxLength={200}
            value={form.customerReviewer}
            onChange={(e) => set("customerReviewer", e.target.value)}
          />
        </label>
        <label>
          结果
          <select
            value={form.result}
            onChange={(e) => set("result", e.target.value)}
          >
            <option
              value="passed"
              disabled={!canAccept || !project.sales_order_line_id}
            >
              通过并自动记账
            </option>
            <option value="rejected">未通过</option>
          </select>
        </label>
        <label>
          验收说明
          <textarea
            required
            maxLength={4000}
            value={form.note}
            onChange={(e) => set("note", e.target.value)}
          />
        </label>
        <label>
          凭据链接
          <input
            type="url"
            maxLength={2000}
            value={form.evidenceUrl}
            onChange={(e) => set("evidenceUrl", e.target.value)}
          />
        </label>
        <button className="primary" type="submit">
          保存验收结果
        </button>
      </fieldset>
    </form>
  );
}
