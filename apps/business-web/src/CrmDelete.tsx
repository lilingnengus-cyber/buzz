import React from "react";
import type { Opportunity } from "./crm";
import { useCrmDraft } from "./CrmDrawer";
import { useCrmCommand } from "./useCrmCommand";

export function CrmDelete({ item, onDeleted }: {
  item: Opportunity;
  onDeleted: () => void;
}) {
  const [confirm, setConfirm] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const dialog = React.useRef<HTMLDialogElement>(null);
  const draft = useCrmDraft();
  const command = useCrmCommand();
  React.useEffect(() => {
    if (confirm) dialog.current?.showModal();
    else dialog.current?.close();
  }, [confirm]);
  const remove = async () => {
    if (busy) return;
    setBusy(true);
    draft.setBusy(true);
    setError("");
    try {
      await command(`/api/v1/crm/opportunities/${item.id}`, {
        method: "DELETE",
        body: JSON.stringify({ expectedVersion: item.version }),
      });
      draft.saved();
      onDeleted();
    } catch (e) {
      setError(e instanceof Error ? e.message : "删除失败，请重试");
    } finally {
      setBusy(false);
      draft.setBusy(false);
    }
  };
  return <>
    <button className="danger" onClick={() => setConfirm(true)}>删除商机</button>
    <dialog ref={dialog} className="crm-discard-prompt" aria-label="删除商机确认"
      onCancel={(e) => { e.preventDefault(); e.stopPropagation(); if (!busy) setConfirm(false); }}
      onClick={(e) => e.stopPropagation()}>
      <h2>删除商机「{item.title}」？</h2>
      <p>商机及其跟进记录将从列表隐藏。客户、联系人和已创建的订单保留。</p>
      {error && <p role="alert" className="crm-error">{error}</p>}
      <div className="crm-discard-actions">
        <button autoFocus disabled={busy} onClick={() => setConfirm(false)}>取消</button>
        <button className="danger" disabled={busy} onClick={remove}>{busy ? "正在删除…" : "确认删除"}</button>
      </div>
    </dialog>
  </>;
}
