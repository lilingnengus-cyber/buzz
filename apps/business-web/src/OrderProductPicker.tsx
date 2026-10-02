import { useState } from "react";
import type { MasterDataRecord } from "./api";
import { CrmSearchSelect } from "./CrmSearchSelect";

export function OrderProductPicker({ label, value, items, disabled = false, onChange }: {
  label: string;
  value: string;
  items: MasterDataRecord[];
  disabled?: boolean;
  onChange: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const selected = items.find((item) => item.id === value);
  const keyword = query.trim().toLocaleLowerCase();
  const matches = items.filter((item) => `${item.code} ${item.name}`.toLocaleLowerCase().includes(keyword));
  return <div className="order-product-picker">
    <CrmSearchSelect label={label} value={value} disabled={disabled}
      selectedLabel={selected ? `${selected.code} · ${selected.name}` : "请选择商品"}
      query={query} onQuery={setQuery} onChange={onChange} showEmptyHint={false}
      options={matches.map((item) => ({ value: item.id, label: `${item.code} · ${item.name}` }))}>
      {!matches.length && <p role="status" className="crm-hint">没有匹配的商品，请调整名称或编码</p>}
    </CrmSearchSelect>
  </div>;
}
