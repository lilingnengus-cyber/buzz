import React from "react";
import { useCrmDraft } from "./CrmDrawer";
import { useCrmCommand } from "./useCrmCommand";
import { LEAD_STATUSES, type Lead } from "./crmLeads";
export function CrmLeadFollowup({
  item,
  onSaved,
  onCancel,
}: {
  item: Lead;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const draft = useCrmDraft();
  const command = useCrmCommand();
  const lock = React.useRef(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const [status, setStatus] = React.useState<string>(
    item.status === "disqualified" ? "contacting" : item.status,
  );
  return (
    <form
      className="crm-form"
      onChangeCapture={draft.markDirty}
      onSubmit={async (e) => {
        e.preventDefault();
        if (lock.current) return;
        const f = new FormData(e.currentTarget);
        lock.current = true;
        setBusy(true);
        draft.setBusy(true);
        setError("");
        try {
          await command(`/api/v1/crm/leads/${item.id}/followups`, {
            method: "POST",
            body: JSON.stringify({
              expectedVersion: item.version,
              note: f.get("note"),
              status,
              nextAction: f.get("nextAction"),
              nextFollowUp: f.get("nextFollowUp") || null,
              disqualificationReason:
                status === "disqualified" ? f.get("reason") : "",
            }),
          });
          draft.saved();
          onSaved();
        } catch (e) {
          setError(e instanceof Error ? e.message : "跟进保存失败");
        } finally {
          lock.current = false;
          setBusy(false);
          draft.setBusy(false);
        }
      }}
    >
      <fieldset className="crm-edit-fields" disabled={busy}>
        <div className="crm-heading">
          <h2>{item.status === "disqualified" ? "重新跟进" : "记录跟进"}</h2>
          <button type="button" onClick={() => draft.discard(onCancel)}>
            取消
          </button>
        </div>
        {error && (
          <p role="alert" className="crm-error">
            {error}
          </p>
        )}
        <div className="crm-fields">
          <label>
            筛选结果
            <select value={status} onChange={(e) => setStatus(e.target.value)}>
              {Object.entries(LEAD_STATUSES)
                .filter(([k]) => k !== "converted")
                .map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
            </select>
          </label>
          {status === "disqualified" && (
            <label>
              淘汰原因
              <input name="reason" required maxLength={1000} />
            </label>
          )}
          <label className="crm-wide">
            沟通内容
            <textarea name="note" required rows={4} maxLength={4000} />
          </label>
          <label>
            下一步
            <input
              name="nextAction"
              maxLength={500}
              defaultValue={item.nextAction}
            />
          </label>
          <label>
            跟进日期
            <input
              name="nextFollowUp"
              type="date"
              defaultValue={item.nextFollowUp ?? ""}
            />
          </label>
        </div>
        <button className="primary" type="submit">
          {busy ? "保存中…" : "保存跟进"}
        </button>
      </fieldset>
    </form>
  );
}
