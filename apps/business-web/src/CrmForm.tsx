import React from "react";
import { useCrmCommand } from "./useCrmCommand";
import {
  amountMinor,
  CRM_STAGES,
  type CrmOption,
  type CrmStage,
  type Opportunity,
} from "./crm";

export function CrmForm({
  record,
  options,
  onSaved,
  onCancel,
}: {
  record?: Opportunity;
  options: CrmOption[];
  onSaved: (id: string) => void;
  onCancel: () => void;
}) {
  const request = useCrmCommand();
  const entities = options.filter((o) => o.resourceType === "legal_entity");
  const [legal, setLegal] = React.useState(
    record?.legalEntityId ?? (entities.length === 1 ? entities[0].id : ""),
  );
  const units = options.filter((o) => o.resourceType === "business_unit");
  const [unit, setUnit] = React.useState(
    record?.businessUnitId ?? (units.length === 1 ? units[0].id : ""),
  );
  const [customer, setCustomer] = React.useState(record?.customerId ?? "");
  const [company, setCompany] = React.useState(record?.companyName ?? "");
  const [stage, setStage] = React.useState<CrmStage>(record?.stage ?? "new");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const lock = React.useRef(false);
  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (lock.current) return;
    const form = new FormData(event.currentTarget);
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await request<{ id: string }>(
        `/api/v1/crm/opportunities${record ? `/${record.id}` : ""}`,
        {
          method: record ? "PUT" : "POST",
          body: JSON.stringify({
            legalEntityId: legal,
            businessUnitId: unit,
            customerId: customer || null,
            title: form.get("title"),
            companyName: company,
            contactName: form.get("contactName"),
            contactDetails: form.get("contactDetails"),
            stage,
            expectedAmountMinor: amountMinor(String(form.get("amount") ?? "")),
            currency: form.get("currency"),
            nextAction: form.get("nextAction"),
            nextFollowUp: form.get("nextFollowUp") || null,
            expectedVersion: record?.version ?? null,
          }),
        },
      );
      onSaved(result.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "保存失败，请重试");
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  return (
    <form className="crm-form" onSubmit={submit}>
      <div className="crm-heading">
        <h2>{record ? "编辑商机" : "新建商机"}</h2>
        <button type="button" onClick={onCancel} disabled={busy}>
          取消
        </button>
      </div>
      {error && (
        <p className="crm-error" role="alert">
          {error}
        </p>
      )}
      <div className="crm-fields">
        <label>
          商机名称
          <input
            name="title"
            required
            maxLength={160}
            defaultValue={record?.title}
            placeholder="例如：杭州客户采购项目"
          />
        </label>
        <label>
          阶段
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
          法人主体
          <select
            required
            value={legal}
            disabled={!!record}
            onChange={(e) => {
              setLegal(e.target.value);
            }}
          >
            <option value="">请选择</option>
            {entities.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          业务单元
          <select
            required
            value={unit}
            disabled={!!record}
            onChange={(e) => {
              setUnit(e.target.value);
            }}
          >
            <option value="">请选择</option>
            {units.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          关联已有客户
          <select
            value={customer}
            onChange={(e) => {
              setCustomer(e.target.value);
              const selected = options.find((o) => o.id === e.target.value);
              if (selected) setCompany(selected.name);
            }}
          >
            <option value="">暂不关联（潜在客户）</option>
            {options
              .filter((o) => o.resourceType === "customer")
              .map((o) => (
                <option key={o.id} value={o.id}>
                  {o.name} · {o.code}
                </option>
              ))}
          </select>
        </label>
        <label>
          客户公司
          <input
            value={company}
            onChange={(e) => setCompany(e.target.value)}
            required
            maxLength={160}
            placeholder="公司名称"
          />
        </label>
        <label>
          联系人
          <input
            name="contactName"
            maxLength={100}
            defaultValue={record?.contactName}
          />
        </label>
        <label>
          联系方式
          <input
            name="contactDetails"
            maxLength={200}
            defaultValue={record?.contactDetails}
            placeholder="电话、微信或邮箱"
          />
        </label>
        <label>
          预计金额
          <input
            name="amount"
            inputMode="decimal"
            defaultValue={
              record?.expectedAmountMinor == null
                ? ""
                : (record.expectedAmountMinor / 100).toFixed(2)
            }
            placeholder="可暂不填写"
          />
        </label>
        <label>
          币种
          <select name="currency" defaultValue={record?.currency ?? "CNY"}>
            {Array.from(
              new Set(
                ["CNY", "USD", "EUR", record?.currency].filter(
                  (v): v is string => !!v,
                ),
              ),
            ).map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </label>
        <label className="crm-wide">
          下一步
          <input
            name="nextAction"
            maxLength={500}
            defaultValue={record?.nextAction}
            placeholder="例如：向采购负责人发送方案"
          />
        </label>
        <label>
          下次跟进日期
          <input
            type="date"
            name="nextFollowUp"
            defaultValue={record?.nextFollowUp ?? ""}
          />
        </label>
      </div>
      <button className="primary" disabled={busy} type="submit">
        {busy ? "保存中…" : "保存商机"}
      </button>
    </form>
  );
}
