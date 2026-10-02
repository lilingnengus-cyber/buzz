import React from "react";
import { request } from "./api";
import { useCrmDraft } from "./CrmDrawer";
import { CrmFollowupForm } from "./CrmFollowupForm";
import type { Opportunity, CrmDetail } from "./crm";
export function CrmFollowupCreate({
  onSaved,
}: {
  onSaved: () => Promise<void>;
}) {
  const draft = useCrmDraft();
  const [query, setQuery] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [items, setItems] = React.useState<Opportunity[]>([]);
  const [hasMore, setHasMore] = React.useState(false);
  const [selected, setSelected] = React.useState<Opportunity | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState("");
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    const timer = setTimeout(() => {
      request<{ items: Opportunity[]; hasMore: boolean }>(
        `/api/v1/crm/opportunities?${new URLSearchParams({ query: query.trim(), offset: String(offset) })}`,
      )
        .then((result) => {
          if (active) {
            setItems(result.items);
            setHasMore(result.hasMore);
          }
        })
        .catch((e) => {
          if (active) {
            setItems([]);
            setError(e instanceof Error ? e.message : "商机加载失败");
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
  }, [query, offset, revision]);
  const choose = async (id: string) => {
    setLoading(true);
    draft.setBusy(true);
    setError("");
    try {
      const result = await request<CrmDetail>(
        `/api/v1/crm/opportunities/${id}`,
      );
      setSelected(result.item);
    } catch (e) {
      setError(e instanceof Error ? e.message : "商机加载失败");
    } finally {
      setLoading(false);
      draft.setBusy(false);
    }
  };
  return (
    <div>
      {selected ? (
        <>
          <div className="crm-heading">
            <div>
              <h2>{selected.title}</h2>
              <p>
                {selected.companyName} ·{" "}
                {selected.contactName || "未填写联系人"}
              </p>
            </div>
            <button
              onClick={() =>
                draft.discard(() => {
                  draft.saved();
                  setSelected(null);
                })
              }
            >
              更换商机
            </button>
          </div>
          <CrmFollowupForm
            key={selected.id}
            item={selected}
            onRefresh={onSaved}
          />
        </>
      ) : (
        <>
          <label className="crm-search">
            搜索关联商机
            <input
              type="search"
              maxLength={160}
              value={query}
              placeholder="商机、客户或联系人"
              onChange={(e) => {
                setQuery(e.target.value);
                setOffset(0);
              }}
            />
          </label>
          {error && (
            <p role="alert">
              {error}{" "}
              <button onClick={() => setRevision((v) => v + 1)}>重试</button>
            </p>
          )}
          {loading ? (
            <p role="status">正在加载商机…</p>
          ) : (
            <>
              {!error && !items.length && (
                <p>没有符合条件的商机，请调整搜索或先创建商机。</p>
              )}
              {items.map((item) => (
                <button
                  className="crm-register-card"
                  key={item.id}
                  onClick={() => choose(item.id)}
                >
                  {item.title} · {item.companyName} ·{" "}
                  {item.contactName || "未填写联系人"}
                </button>
              ))}
              {!error && (offset > 0 || hasMore) && (
                <nav className="crm-pagination" aria-label="选择商机分页">
                  <button
                    disabled={offset === 0}
                    onClick={() => setOffset(offset - 50)}
                  >
                    上一页
                  </button>
                  <button
                    disabled={!hasMore}
                    onClick={() => setOffset(offset + 50)}
                  >
                    下一页
                  </button>
                </nav>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
