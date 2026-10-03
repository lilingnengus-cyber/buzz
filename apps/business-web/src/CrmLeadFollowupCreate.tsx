import React from "react";
import { request } from "./api";
import { CrmSearchSelect } from "./CrmSearchSelect";
import { CrmLeadFollowup } from "./CrmLeadFollowup";
import type { Lead, LeadDetail } from "./crmLeads";
export function CrmLeadFollowupCreate({
  onSaved,
}: {
  onSaved: () => Promise<void>;
}) {
  const [query, setQuery] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [items, setItems] = React.useState<Lead[]>([]);
  const [more, setMore] = React.useState(false);
  const [selected, setSelected] = React.useState<Lead | null>(null);
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    let active = true;
    setBusy(true);
    setItems([]);
    setError("");
    const timer = setTimeout(() => {
      request<{ items: Lead[]; hasMore: boolean }>(
        `/api/v1/crm/leads?${new URLSearchParams({ query, offset: String(offset) })}`,
      )
        .then((r) => {
          if (active) {
            setItems(r.items.filter((l) => l.status !== "converted"));
            setMore(r.hasMore);
          }
        })
        .catch((e) => {
          if (active) {
            setError(e.message);
            setItems([]);
          }
        })
        .finally(() => {
          if (active) setBusy(false);
        });
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [query, offset, revision]);
  if (selected)
    return (
      <CrmLeadFollowup
        item={selected}
        onSaved={() => void onSaved()}
        onCancel={() => setSelected(null)}
      />
    );
  return (
    <div className="crm-form">
      <CrmSearchSelect
        label="关联线索"
        value=""
        query={query}
        onQuery={(q) => {
          setQuery(q);
          setOffset(0);
        }}
        options={items.map((l) => ({
          value: l.id,
          label: `${l.title} · ${l.companyName || "公司待确认"}`,
        }))}
        onChange={(id) => {
          setBusy(true);
          setError("");
          request<LeadDetail>(`/api/v1/crm/leads/${id}`)
            .then((r) => setSelected(r.item))
            .catch((e) => setError(e.message))
            .finally(() => setBusy(false));
        }}
        selectedLabel="请选择线索"
      />
      {busy && <p role="status">正在读取线索…</p>}
      {error && (
        <p role="alert">
          {error}
          <button onClick={() => setRevision((v) => v + 1)}>重试</button>
        </p>
      )}
      {(offset > 0 || more) && (
        <nav className="crm-pagination">
          <button
            disabled={busy || offset === 0}
            onClick={() => setOffset(offset - 50)}
          >
            上一页
          </button>
          <button
            disabled={busy || !more}
            onClick={() => setOffset(offset + 50)}
          >
            下一页
          </button>
        </nav>
      )}
    </div>
  );
}
