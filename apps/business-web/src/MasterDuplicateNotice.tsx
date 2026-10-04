import React from "react";
import type { CoreMasterRecord } from "./api";
import { loadAllMasterData } from "./masterDataPages";

const normalize = (name: string) => name.normalize("NFKC").replace(/\s+/gu, "").toLocaleLowerCase();

/** Advisory duplicate detection within the current user's authorized records. */
export function MasterDuplicateNotice({ type, name, id }: { type: "customer" | "supplier"; name: string; id?: string }) {
  const [items, setItems] = React.useState<CoreMasterRecord[] | null>(null);
  const [failed, setFailed] = React.useState(false);
  const [revision, retry] = React.useState(0);
  React.useEffect(() => {
    const controller = new AbortController();
    setItems(null);
    setFailed(false);
    loadAllMasterData<CoreMasterRecord>("/api/v1/core-master-data", type, controller.signal)
      .then((page) => { if (!controller.signal.aborted) setItems(page.items); })
      .catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, [type, revision]);
  const key = normalize(name);
  if (!key) return null;
  if (failed) return <div role="status" className="master-note">暂未完成重复检查，请自行核对。<button type="button" onClick={() => retry((n) => n + 1)}>重试重复检查</button></div>;
  if (!items) return <p role="status">正在检查可能重复的资料…</p>;
  const matches = items.filter((item) => {
    if (item.id === id || item.resourceType !== type) return false;
    const other = normalize(item.name);
    return other === key || (Math.min(key.length, other.length) >= 4 && (other.includes(key) || key.includes(other)));
  }).sort((a, b) => Number(normalize(b.name) === key) - Number(normalize(a.name) === key) || a.code.localeCompare(b.code));
  if (!matches.length) return null;
  return <div role="status" className="master-note" style={{ gridColumn: "1 / -1" }}>
    <strong>发现 {matches.length} 条可能重复的{type === "customer" ? "客户" : "供应商"}</strong>
    <p>请核对名称和编码，确认是不同对象后仍可保存。</p>
    <ul>{matches.slice(0, 5).map((item) => <li key={item.id}>{item.code} · {item.name} · {normalize(item.name) === key ? "同名" : "名称相近"}{item.status === "disabled" ? " · 已停用" : ""}</li>)}</ul>
    {matches.length > 5 && <p>仅展示前 5 条，请在列表中进一步核对。</p>}
    <small>仅检查当前可见的同类资料，提示不代表唯一性校验。</small>
  </div>;
}
