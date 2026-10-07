import "./crm-registers.css";
import "./crm-opportunities.css";
import React from "react";
import { request, toApiFailure, type ApiFailure } from "./api";
import { PageLoadFailure } from "./PageLoadFailure";
import { CrmDirectoryDetail } from "./CrmRecordDetails";
import { CrmDrawer, useCrmDraft } from "./CrmDrawer";
import { CrmAccountPicker } from "./CrmDirectoryFields";
import { useCrmCommand } from "./useCrmCommand";
import type { CrmAccount, CrmContact, CrmOption } from "./crm";
import "./crm.css";
import "./crm-opportunities.css";
type Directory = {
  items: (CrmAccount | CrmContact)[];
  hasMore: boolean;
  canManage: boolean;
};
function DirectoryForm({
  kind,
  item,
  onSaved,
}: {
  kind: "accounts" | "contacts";
  item: CrmAccount | CrmContact | null;
  onSaved: () => void;
}) {
  const draft = useCrmDraft(),
    command = useCrmCommand();
  const accountItem = item && "name" in item ? item : null;
  const contactItem = item && "accountId" in item ? item : null;
  const [account, setAccount] = React.useState<CrmAccount | null>(
    contactItem
      ? {
          id: contactItem.accountId,
          name: contactItem.companyName,
          customerId: null,
          version: 0,
        }
      : null,
  );
  const [choices, setChoices] = React.useState<CrmOption[]>([]);
  const [customer, setCustomer] = React.useState(accountItem?.customerId ?? "");
  const [name, setName] = React.useState(
    accountItem?.name ?? contactItem?.contactName ?? "",
  );
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const lock = React.useRef(false);
  React.useEffect(() => {
    if (kind !== "accounts" || item) return;
    let active = true;
    request<{ items: CrmOption[] }>("/api/v1/crm/options")
      .then((data) => {
        if (active)
          setChoices(data.items.filter((o) => o.resourceType === "customer"));
      })
      .catch(() => {
        if (active) setError("已有客户加载失败，可稍后重试");
      });
    return () => {
      active = false;
    };
  }, [kind, item]);
  return (
    <form
      className="crm-form"
      onChangeCapture={(event) => {
        if (
          !(
            event.target instanceof HTMLInputElement &&
            event.target.type === "search"
          )
        )
          draft.markDirty();
      }}
      onSubmit={async (e) => {
        e.preventDefault();
        if (lock.current) return;
        if (kind === "contacts" && !account) {
          setError("请选择客户档案");
          return;
        }
        const fields = new FormData(e.currentTarget);
        lock.current = true;
        setBusy(true);
        draft.setBusy(true);
        setError("");
        try {
          await command(`/api/v1/crm/${kind}${item ? `/${item.id}` : ""}`, {
            method: item ? "PUT" : "POST",
            body: JSON.stringify(
              kind === "accounts"
                ? {
                    name,
                    customerId: customer || null,
                    expectedVersion: item?.version ?? null,
                  }
                : {
                    name,
                    accountId: account?.id,
                    details: fields.get("details"),
                    expectedVersion: item?.version ?? null,
                  },
            ),
          });
          draft.saved();
          onSaved();
        } catch (error) {
          setError(error instanceof Error ? error.message : "保存失败，请重试");
        } finally {
          lock.current = false;
          setBusy(false);
          draft.setBusy(false);
        }
      }}
    >
      <fieldset className="crm-edit-fields" disabled={busy}>
        {error && (
          <p className="crm-error" role="alert">
            {error}
          </p>
        )}
        {kind === "accounts" ? (
          <>
            {!item && (
              <label>
                关联核心客户
                <select
                  value={customer}
                  onChange={(e) => {
                    setCustomer(e.target.value);
                    setName(
                      choices.find((o) => o.id === e.target.value)?.name ?? "",
                    );
                  }}
                >
                  <option value="">潜在客户（仅需名称）</option>
                  {choices.map((o) => (
                    <option key={o.id} value={o.id}>
                      {o.name} · {o.code}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label>
              客户名称
              <input
                required
                maxLength={160}
                value={name}
                readOnly={Boolean(customer)}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            {customer && (
              <p className="crm-hint">
                名称复用核心客户资料；修改名称请前往核心数据。
              </p>
            )}
          </>
        ) : (
          <>
            {item ? (
              <p>所属客户：{contactItem?.companyName}</p>
            ) : (
              <CrmAccountPicker
                value={account}
                onChange={(value) => {
                  setAccount(value);
                  draft.markDirty();
                }}
              />
            )}
            <label>
              联系人姓名
              <input
                required
                maxLength={100}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label>
              联系方式
              <input
                name="details"
                maxLength={200}
                defaultValue={contactItem?.contactDetails}
                placeholder="电话、微信或邮箱"
              />
            </label>
            <p className="crm-hint">保存后，关联商机统一显示最新联系人资料。</p>
          </>
        )}
        <button className="primary" disabled={busy}>
          {busy ? "保存中…" : "保存档案"}
        </button>
      </fieldset>
    </form>
  );
}
export function CrmDirectoryPage() {
  const [filterAccount, setFilterAccount] = React.useState<CrmAccount | null>(
    null,
  );
  const [kind] = React.useState<"accounts" | "contacts">("contacts");
  const [query, setQuery] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [revision, setRevision] = React.useState(0);
  const [data, setData] = React.useState<Directory>({
    items: [],
    hasMore: false,
    canManage: false,
  });
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  const [editing, setEditing] = React.useState<{
    item: CrmAccount | CrmContact | null;
    details?: boolean;
  } | null>(null);
  React.useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    const timer = setTimeout(() => {
      request<Directory>(
        `/api/v1/crm/${kind}?query=${encodeURIComponent(query.trim())}&offset=${offset}${kind === "contacts" && filterAccount ? `&accountId=${encodeURIComponent(filterAccount.id)}` : ""}`,
      )
        .then((result) => {
          if (active) setData(result);
        })
        .catch((e) => {
          if (active) {
            setData({ items: [], hasMore: false, canManage: false });
            setError(toApiFailure(e, "档案加载失败"));
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
  }, [kind, query, offset, revision, filterAccount]);
  const title = kind === "accounts" ? "客户" : "联系人";
  return (
    <section className="crm-page crm-opportunities crm-directory-page">
      <header className="crm-heading">
        <div>
          <p className="eyebrow">售前 CRM</p>
          <h1>联系人</h1>
          <p className="crm-hint">
            统一维护客户联系人，多个商机共同引用。正式客户在核心数据中维护。
          </p>
        </div>
        {data.canManage && (
          <button
            className="primary"
            onClick={() => setEditing({ item: null })}
          >
            新建{title}
          </button>
        )}
      </header>
      <div className="crm-toolbar">
        <label className="crm-search">
          搜索{title}
          <input
            type="search"
            value={query}
            maxLength={160}
            onChange={(e) => {
              setQuery(e.target.value);
              setOffset(0);
            }}
            placeholder={
              kind === "accounts" ? "客户名称" : "客户、姓名或联系方式"
            }
          />
        </label>
        <button onClick={() => setRevision((v) => v + 1)}>刷新</button>
      </div>
      {kind === "contacts" && (
        <CrmAccountPicker
          filter
          value={filterAccount}
          onChange={(value) => {
            setFilterAccount(value);
            setOffset(0);
          }}
        />
      )}
      {error && <PageLoadFailure failure={error} resourceLabel="联系人" onRetry={() => setRevision((v) => v + 1)} />}
      <div className="crm-register" aria-busy={loading}>
        <div className="crm-list-caption">
          <strong>{title}档案</strong>
          <span>
            {loading ? "加载中…" : `本页 ${data.items.length} 条档案`} ·
            点击记录查看详情
          </span>
        </div>
        <div
          className={`crm-register-columns ${kind === "contacts" ? "crm-contact-grid" : "crm-account-grid"}`}
          aria-hidden="true"
        >
          {kind === "contacts" ? (
            <>
              <span>联系人</span>
              <span>所属客户</span>
              <span>联系方式</span>
              <span>关联商机</span>
              <span>操作</span>
            </>
          ) : (
            <>
              <span>客户名称</span>
              <span>客户类型</span>
              <span>操作</span>
            </>
          )}
        </div>
        {loading ? (
          <p className="crm-empty" role="status">
            正在加载档案…
          </p>
        ) : !error && !data.items.length ? (
          <p className="crm-empty">
            {query
              ? "没有符合条件的档案"
              : `还没有${title}，可先建立客户，再添加联系人。`}
          </p>
        ) : (
          data.items.map((item) => (
            <article
              key={item.id}
              className={`crm-register-card crm-clickable-record ${"contactName" in item ? "crm-contact-grid" : "crm-account-grid"}`}
              role="button"
              tabIndex={0}
              aria-haspopup="dialog"
              aria-label={`查看${title}：${"contactName" in item ? item.contactName : item.name}`}
              onClick={(event) => {
                if (
                  !(
                    event.target instanceof Element &&
                    event.target.closest("a, button")
                  )
                )
                  setEditing({ item, details: true });
              }}
              onKeyDown={(event) => {
                if (
                  event.target === event.currentTarget &&
                  ["Enter", " "].includes(event.key)
                ) {
                  event.preventDefault();
                  setEditing({ item, details: true });
                }
              }}
            >
              {"contactName" in item ? (
                <>
                  <div>
                    <h2>{item.contactName}</h2>
                  </div>
                  <div className="crm-register-cell">
                    <span className="crm-mobile-label">所属客户</span>
                    {item.customerId ? (
                      <a
                        href={`/customers/${encodeURIComponent(item.customerId)}`}
                      >
                        <strong>{item.companyName}</strong>
                      </a>
                    ) : (
                      <strong>{item.companyName}</strong>
                    )}
                  </div>
                  <div className="crm-register-cell">
                    <span className="crm-mobile-label">联系方式</span>
                    <span>{item.contactDetails || "未填写"}</span>
                  </div>
                  <div className="crm-related crm-register-cell">
                    <span className="crm-mobile-label">关联商机</span>
                    {item.opportunities.length ? (
                      item.opportunities.map((o) => (
                        <a
                          key={o.id}
                          href={`/#crm?opportunity=${encodeURIComponent(o.id)}`}
                        >
                          {o.title}
                        </a>
                      ))
                    ) : (
                      <span className="crm-hint">尚未关联</span>
                    )}
                  </div>
                  <div className="crm-register-actions">
                    {data.canManage && (
                      <button onClick={() => setEditing({ item })}>
                        编辑联系人
                      </button>
                    )}
                  </div>
                </>
              ) : (
                <>
                  <div>
                    <h2>{item.name}</h2>
                  </div>
                  <div>
                    <span
                      className={`crm-stage ${item.customerId ? "crm-stage-won" : "crm-stage-new"}`}
                    >
                      {item.customerId ? "已关联核心客户" : "潜在客户"}
                    </span>
                  </div>
                  <div className="crm-register-actions">
                    {data.canManage && !item.customerId && (
                      <button onClick={() => setEditing({ item })}>
                        编辑客户
                      </button>
                    )}
                  </div>
                </>
              )}
            </article>
          ))
        )}
      </div>
      {(offset > 0 || data.hasMore) && (
        <nav className="crm-pagination" aria-label="档案分页">
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
      {editing && (
        <CrmDrawer
          title={
            editing.details
              ? `${editing.item && "contactName" in editing.item ? "联系人" : title}详情`
              : `${editing.item ? "编辑" : "新建"}${editing.item && "contactName" in editing.item ? "联系人" : title}`
          }
          onClose={() => setEditing(null)}
        >
          {editing.details && editing.item ? (
            <CrmDirectoryDetail
              item={editing.item}
              canManage={data.canManage}
              onContact={(item) => setEditing({ item, details: true })}
              onEdit={() => setEditing({ item: editing.item })}
            />
          ) : (
            <DirectoryForm
              kind={
                editing.item && "contactName" in editing.item
                  ? "contacts"
                  : kind
              }
              item={editing.item}
              onSaved={() => {
                setEditing(null);
                setRevision((v) => v + 1);
              }}
            />
          )}
        </CrmDrawer>
      )}
    </section>
  );
}
