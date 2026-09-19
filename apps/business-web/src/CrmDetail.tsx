import React from "react";
import { request as read } from "./api";
import { useCrmCommand } from "./useCrmCommand";
import { formatMoney } from "./formatters";
import { CRM_STAGES, type CrmDetail as Detail, type CrmStage } from "./crm";
export function CrmDetail({
  data,
  canManage,
  onEdit,
  onRefresh,
}: {
  data: Detail;
  canManage: boolean;
  onEdit: () => void;
  onRefresh: () => Promise<void>;
}) {
  const request = useCrmCommand();
  const item = data.item;
  const [history, setHistory] = React.useState(data.followups);
  const [hasOlder, setHasOlder] = React.useState(data.hasOlderFollowups);
  const [historyLoading, setHistoryLoading] = React.useState(false);
  const older = async () => {
    setHistoryLoading(true);
    setError("");
    try {
      const next = await read<Detail>(
        `/api/v1/crm/opportunities/${item.id}?offset=${history.length}`,
      );
      setHistory((previous) => [
        ...previous,
        ...next.followups.filter((n) => !previous.some((p) => p.id === n.id)),
      ]);
      setHasOlder(next.hasOlderFollowups);
    } catch (e) {
      setError(e instanceof Error ? e.message : "跟进记录加载失败");
    } finally {
      setHistoryLoading(false);
    }
  };
  const [stage, setStage] = React.useState<CrmStage>(item.stage);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const lock = React.useRef(false);
  const submit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (lock.current) return;
    const form = e.currentTarget;
    const fields = new FormData(form);
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      await request(`/api/v1/crm/opportunities/${item.id}/followups`, {
        method: "POST",
        body: JSON.stringify({
          note: fields.get("note"),
          stage,
          nextAction: fields.get("nextAction"),
          nextFollowUp: fields.get("nextFollowUp") || null,
          expectedVersion: item.version,
        }),
      });
      await onRefresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "跟进保存失败，请重试");
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  return (
    <div className="crm-detail">
      <div className="crm-heading">
        <div>
          <span className={`crm-stage crm-stage-${item.stage}`}>
            {CRM_STAGES[item.stage]}
          </span>
          <h2>{item.title}</h2>
          <p>{item.companyName}</p>
        </div>
        {canManage && <button onClick={onEdit}>编辑商机</button>}
      </div>
      <dl className="crm-facts">
        <div>
          <dt>联系人</dt>
          <dd>{item.contactName || "未填写"}</dd>
        </div>
        <div>
          <dt>联系方式</dt>
          <dd>{item.contactDetails || "未填写"}</dd>
        </div>
        <div>
          <dt>预计金额</dt>
          <dd>
            {item.expectedAmountMinor == null
              ? "待确认"
              : formatMoney(item.currency, item.expectedAmountMinor / 100)}
          </dd>
        </div>
        <div>
          <dt>下次跟进</dt>
          <dd>{item.nextFollowUp || "未安排"}</dd>
        </div>
      </dl>
      <div className="crm-next">
        <strong>下一步</strong>
        <p>{item.nextAction || "记录一次跟进，安排下一步。"}</p>
      </div>
      {item.stage === "won" && (
        <p className="crm-hint">
          已成交。需要录单时，请前往<a href="#sales">销售订单</a>。
        </p>
      )}
      {canManage && (
        <form className="crm-form" onSubmit={submit}>
          <h3>记录跟进</h3>
          {error && (
            <p role="alert" className="crm-error">
              {error}
            </p>
          )}
          <label>
            本次沟通
            <textarea
              name="note"
              required
              maxLength={4000}
              rows={3}
              placeholder="客户反馈、已确认事项…"
            />
          </label>
          <div className="crm-fields">
            <label>
              更新阶段
              <select
                value={stage}
                onChange={(e) => setStage(e.target.value as CrmStage)}
              >
                {Object.entries(CRM_STAGES).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
            <label>
              下次跟进日期
              <input
                type="date"
                name="nextFollowUp"
                defaultValue={item.nextFollowUp ?? ""}
              />
            </label>
          </div>
          <label>
            下一步
            <input
              name="nextAction"
              maxLength={500}
              defaultValue={item.nextAction}
              placeholder="明确下一步要做什么"
            />
          </label>
          <button className="primary" disabled={busy}>
            {busy ? "保存中…" : "保存跟进"}
          </button>
        </form>
      )}
      <section className="crm-history">
        <h3>
          跟进记录 <span>{history.length}</span>
        </h3>
        {!history.length && <p className="crm-hint">还没有跟进记录。</p>}
        {history.map((note) => (
          <article key={note.id}>
            <div className="crm-note-meta">
              <strong>{note.authorName}</strong>
              <time>{new Date(note.createdAt).toLocaleString("zh-CN")}</time>
              <span>{CRM_STAGES[note.stage]}</span>
            </div>
            <p>{note.note}</p>
            {note.nextAction && (
              <p className="crm-hint">
                下一步：{note.nextAction}
                {note.nextFollowUp && ` · ${note.nextFollowUp}`}
              </p>
            )}
          </article>
        ))}
        {hasOlder && (
          <button disabled={historyLoading} onClick={older}>
            {historyLoading ? "加载中…" : "查看更早的跟进"}
          </button>
        )}
      </section>
    </div>
  );
}
