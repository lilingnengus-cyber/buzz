import React from "react";
import { CrmConversionFields } from "./CrmConversionFields";
import { useCrmDraft } from "./CrmDrawer";
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
  const draft = useCrmDraft();
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
    draft.setBusy(true);
    setError("");
    try {
      const conversion = stage === "won";
      await request(
        `/api/v1/crm/opportunities/${item.id}/${conversion ? "convert-customer" : "followups"}`,
        {
          method: "POST",
          body: JSON.stringify({
            ...(conversion
              ? {
                  customerId: fields.get("confirmedCustomerId") || null,
                  customerName:
                    fields.get("conversionCustomerName") || item.companyName,
                  contactName: fields.get("conversionContactName"),
                  contactDetails: fields.get("conversionContactDetails"),
                  creditCurrency:
                    fields.get("conversionCurrency") || item.currency,
                  paymentTermsDays: Number(fields.get("conversionTerms") ?? 30),
                }
              : {
                  stage,
                  nextAction: fields.get("nextAction"),
                  nextFollowUp: fields.get("nextFollowUp") || null,
                  lossReason: stage === "lost" ? fields.get("lossReason") : "",
                }),
            note: fields.get("note"),

            expectedVersion: item.version,
          }),
        },
      );
      draft.saved();
      await onRefresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "跟进保存失败，请重试");
    } finally {
      lock.current = false;
      setBusy(false);
      draft.setBusy(false);
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
        {canManage && (
          <button
            onClick={() => {
              draft.discard(onEdit);
            }}
          >
            编辑商机
          </button>
        )}
      </div>
      <dl className="crm-facts">
        <div>
          <dt>商机负责人</dt>
          <dd>{item.ownerName || "未设置"}</dd>
        </div>
        <div>
          <dt>预计成交日期</dt>
          <dd>{item.expectedCloseDate || "未安排"}</dd>
        </div>
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
      {item.stage === "lost" && (
        <div className="crm-next">
          <strong>流失原因</strong>
          <p>{item.lossReason || "历史记录未填写"}</p>
        </div>
      )}
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
        <form
          className="crm-form"
          onChangeCapture={(event) => {
            if (
              !(
                event.target instanceof HTMLInputElement &&
                event.target.type === "search"
              )
            )
              draft.markDirty();
          }}
          onSubmit={submit}
        >
          <fieldset className="crm-edit-fields" disabled={busy}>
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
            {stage === "won" && (
              <CrmConversionFields key={item.id} item={item} />
            )}
            {stage === "lost" && (
              <label>
                流失原因
                <textarea
                  name="lossReason"
                  required
                  maxLength={1000}
                  rows={3}
                  defaultValue={item.lossReason ?? ""}
                  placeholder="说明本次商机流失的主要原因"
                />
              </label>
            )}
            <button className="primary" disabled={busy}>
              {busy
                ? "保存中…"
                : stage === "won"
                  ? "确认成交并保存档案"
                  : "保存跟进"}
            </button>
          </fieldset>
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
            {note.lossReason && (
              <p className="crm-hint">流失原因：{note.lossReason}</p>
            )}
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
