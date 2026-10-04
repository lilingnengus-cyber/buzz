import { useState } from "react";
import { CrmSearchSelect } from "./CrmSearchSelect";

type Choice = { id: string; code: string; name: string };
export function MasterSearchSelect({ label, value, items, onChange, disabled = false, required = false, emptyLabel = "请选择", fallbackLabel }: {
  label: string; value: string; items: Choice[]; onChange: (id: string) => void;
  disabled?: boolean; required?: boolean; emptyLabel?: string; fallbackLabel?: string;
}) {
  const [query, setQuery] = useState("");
  const selected = items.find((item) => item.id === value);
  const matches = items.filter((item) => `${item.code} ${item.name}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <div className="master-search-picker" data-order-required={required ? label.replace(/\s*\*$/, "") : undefined} data-order-value={value}>
    <CrmSearchSelect label={label} value={value} disabled={disabled} query={query} onQuery={setQuery} onChange={onChange}
      selectedLabel={selected ? `${selected.code} · ${selected.name}` : value ? (fallbackLabel || value) : emptyLabel}
      showEmptyHint={false} options={[...(!required ? [{ value: "", label: emptyLabel }] : []), ...matches.map((item) => ({ value: item.id, label: `${item.code} · ${item.name}` }))]}>
      {!matches.length && <p role="status">没有匹配资料，请调整名称或编码</p>}
    </CrmSearchSelect>
  </div>;
}
