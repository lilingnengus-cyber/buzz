import React from "react";
import { useCrmDraft } from "./CrmDrawer";
import { useCrmCommand } from "./useCrmCommand";
import { CrmConversionFields } from "./CrmConversionFields";
import { CRM_STAGES, type Opportunity, type CrmStage } from "./crm";
export function CrmFollowupForm({
  item,
  onRefresh,
}: {
  item: Opportunity;
  onRefresh: () => Promise<void>;
}) {
  const request = useCrmCommand();
  const draft = useCrmDraft();
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
          {stage !== "won" && (
            <label>
              下次跟进日期
              <input
                type="date"
                name="nextFollowUp"
                defaultValue={item.nextFollowUp ?? ""}
              />
            </label>
          )}
        </div>
        {stage !== "won" && (
          <label>
            下一步
            <input
              name="nextAction"
              maxLength={500}
              defaultValue={item.nextAction}
              placeholder="明确下一步要做什么"
            />
          </label>
        )}
        {stage === "won" && <CrmConversionFields key={item.id} item={item} />}
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
  );
}
