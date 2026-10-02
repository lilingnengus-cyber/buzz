import { useOrderDraft } from "./OrderDraft";
import { useState } from "react";
import type { MasterDataRecord } from "./api";
import { CrmSearchSelect } from "./CrmSearchSelect";

export function OrderMasterPicker({ label, value, items, disabled = false, noun = "商品", inLine = true, placeholder, onChange }: {
  noun?: string;
  inLine?: boolean;
  placeholder?: string;
  label: string;
  value: string;
  items: MasterDataRecord[];
  disabled?: boolean;
  onChange: (id: string) => void;
}) {
  const draft = useOrderDraft();
  const [query, setQuery] = useState("");
  const selected = items.find((item) => item.id === value);
  const keyword = query.trim().toLocaleLowerCase();
  const matches = items.filter((item) => `${item.code} ${item.name}`.toLocaleLowerCase().includes(keyword));
  return <div data-order-required={label} data-order-value={value} className={inLine ? "order-product-picker" : "entry-field"}>
    <CrmSearchSelect label={label} value={value} disabled={disabled}
      selectedLabel={selected ? `${selected.code} · ${selected.name}` : (placeholder ?? `请选择${noun}`)}
      query={query} onQuery={setQuery} onChange={(id) => { if (id !== value) draft.markDirty(); onChange(id); }} showEmptyHint={false}
      options={matches.map((item) => ({ value: item.id, label: `${item.code} · ${item.name}` }))}>
      {!matches.length && <p role="status" className="crm-hint">没有匹配的{noun}，请调整名称或编码</p>}
    </CrmSearchSelect>
  </div>;
}
