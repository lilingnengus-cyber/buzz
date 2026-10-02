import { CrmSearchSelect } from "./CrmSearchSelect";
import React from "react";
import { request } from "./api";
import type { CrmAccount, CrmContact } from "./crm";
export function CrmAccountPicker({
  value,
  onChange,
  filter = false,
}: {
  filter?: boolean;
  value: CrmAccount | null;
  onChange: (value: CrmAccount | null) => void;
}) {
  const [query, setQuery] = React.useState("");
  const [items, setItems] = React.useState<CrmAccount[]>([]);
  const [error, setError] = React.useState("");
  const [more, setMore] = React.useState(false);
  React.useEffect(() => {
    let active = true;
    const timer = setTimeout(() => {
      request<{ items: CrmAccount[]; hasMore: boolean }>(
        `/api/v1/crm/accounts?query=${encodeURIComponent(query)}`,
      )
        .then((data) => {
          if (active) {
            setItems(data.items);
            setMore(data.hasMore);
            setError("");
          }
        })
        .catch(() => {
          if (active) {
            setItems([]);
            setError("客户档案加载失败，请调整搜索重试");
          }
        });
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [query]);
  const choices =
    value && !items.some((item) => item.id === value.id)
      ? [value, ...items]
      : items;
  return (
    <div
      className={`crm-wide crm-directory-picker ${filter ? "crm-customer-filter" : ""}`}
    >
      <CrmSearchSelect label={filter ? "按客户筛选" : "客户档案"}
        value={value?.id ?? ""} query={query} onQuery={setQuery}
        onChange={(id) => onChange(choices.find((item) => item.id === id) ?? null)}
        options={[{ value: "", label: filter ? "全部客户" : "快速填写 / 暂不选择" },
          ...choices.map((item) => ({ value: item.id, label: item.name + (item.customerId ? "" : "（潜在客户）") }))]} />
      {more && (
        <p className="crm-hint">匹配超过 50 个客户，请输入更完整的名称。</p>
      )}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
export function CrmContactPicker({
  accountId,
  value,
  onChange,
}: {
  accountId: string;
  value: CrmContact | null;
  onChange: (value: CrmContact | null) => void;
}) {
  const [query, setQuery] = React.useState("");
  const [items, setItems] = React.useState<CrmContact[]>([]);
  const [error, setError] = React.useState("");
  const [more, setMore] = React.useState(false);
  React.useEffect(() => {
    let active = true;
    const timer = setTimeout(() => {
      request<{ items: CrmContact[]; hasMore: boolean }>(
        `/api/v1/crm/contacts?accountId=${encodeURIComponent(accountId)}&query=${encodeURIComponent(query)}`,
      )
        .then((data) => {
          if (active) {
            setItems(data.items);
            setMore(data.hasMore);
            setError("");
          }
        })
        .catch(() => {
          if (active) {
            setItems([]);
            setError("联系人加载失败，请调整搜索重试");
          }
        });
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [accountId, query]);
  const choices =
    value && !items.some((item) => item.id === value.id)
      ? [value, ...items]
      : items;
  return (
    <div className="crm-wide crm-directory-picker">
      <CrmSearchSelect label="选择联系人" value={value?.id ?? ""} query={query} onQuery={setQuery}
        onChange={(id) => onChange(choices.find((item) => item.id === id) ?? null)}
        options={[{ value: "", label: "填写新联系人 / 暂不填写" },
          ...choices.map((item) => ({ value: item.id, label: `${item.contactName} · ${item.contactDetails || "未填写联系方式"}` }))]} />
      {more && <p className="crm-hint">匹配超过 50 人，请继续搜索。</p>}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
