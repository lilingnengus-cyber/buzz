import React from "react";
import { request } from "./api";
export function CrmOwnerPicker({
  legal,
  unit,
  customer,
  value,
  currentName,
  onChange,
}: {
  legal: string;
  unit: string;
  customer: string;
  value: string;
  currentName?: string;
  onChange: (value: string) => void;
}) {
  const [selectedName, setSelectedName] = React.useState(
    currentName ?? "当前负责人",
  );
  const [query, setQuery] = React.useState("");
  const [items, setItems] = React.useState<{ id: string; name: string }[]>([]);
  const [more, setMore] = React.useState(false);
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    let active = true;
    setItems([]);
    setError("");
    setMore(false);
    if (!legal || !unit) return;
    const timer = setTimeout(() => {
      const params = new URLSearchParams({
        legalEntityId: legal,
        businessUnitId: unit,
        query,
      });
      if (customer) params.set("customerId", customer);
      request<{ items: { id: string; name: string }[]; hasMore: boolean }>(
        `/api/v1/crm/owners?${params}`,
      )
        .then((data) => {
          if (active) {
            setItems(data.items);
            setMore(data.hasMore);
          }
        })
        .catch(() => {
          if (active) setError("负责人列表加载失败，请调整搜索重试。");
        });
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [legal, unit, customer, query]);
  return (
    <div className="crm-directory-picker crm-wide">
      <label>
        搜索负责人
        <input
          type="search"
          value={query}
          maxLength={100}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="同事姓名"
        />
      </label>
      <label>
        商机负责人
        <select
          aria-label="商机负责人"
          disabled={!legal || !unit}
          value={value}
          onChange={(e) => {
            setSelectedName(
              items.find((item) => item.id === e.target.value)?.name ??
                currentName ??
                "当前负责人",
            );
            onChange(e.target.value);
          }}
        >
          <option value="">{currentName ? "保持原负责人" : "当前账号"}</option>
          {value && !items.some((item) => item.id === value) && (
            <option value={value}>{selectedName}（当前选择）</option>
          )}
          {items.map((item) => (
            <option value={item.id} key={item.id}>
              {item.name}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <p role="alert" className="crm-error">
          {error}
        </p>
      )}
      {more && <p className="crm-hint">候选范围较大，请输入完整姓名查找。</p>}
      {!legal || !unit ? (
        <p className="crm-hint">选择业务主体后可分配负责人。</p>
      ) : (
        <p className="crm-hint">仅列出有权处理当前商机范围的同事。</p>
      )}
    </div>
  );
}
