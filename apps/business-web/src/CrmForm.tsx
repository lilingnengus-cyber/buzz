import React from "react";
import { CrmAccountPicker, CrmContactPicker } from "./CrmDirectoryFields";
import type { CrmAccount, CrmContact } from "./crm";
import { useCrmDraft } from "./CrmDrawer";
import { OperatingUnitPicker } from "./OperatingUnitPicker";
import {
  rememberSyncedRecentOperatingUnit,
  resolveRecentOperatingUnit,
} from "./recentOperatingUnit";
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
  const draft = useCrmDraft();
  const entities = options.filter((o) => o.resourceType === "legal_entity");
  const [legal, setLegal] = React.useState(
    record?.legalEntityId ?? (entities.length === 1 ? entities[0].id : ""),
  );
  const units = options.filter((o) => o.resourceType === "business_unit");
  const [unit, setUnit] = React.useState(
    record?.businessUnitId ??
      resolveRecentOperatingUnit(
        "crm-opportunity",
        units,
        units.length === 1 ? units[0].id : "",
      ),
  );
  const [customer, setCustomer] = React.useState(record?.customerId ?? "");
  const [account, setAccount] = React.useState<CrmAccount | null>(
    record?.accountId
      ? {
          id: record.accountId,
          customerId: record.customerId,
          name: record.companyName,
          version: 0,
        }
      : null,
  );
  const [contact, setContact] = React.useState<CrmContact | null>(
    record?.contactId && record.accountId
      ? {
          id: record.contactId,
          accountId: record.accountId,
          companyName: record.companyName,
          contactName: record.contactName,
          contactDetails: record.contactDetails,
          version: 0,
          opportunities: [],
        }
      : null,
  );
  const [contactName, setContactName] = React.useState(
    record?.contactName ?? "",
  );
  const [contactDetails, setContactDetails] = React.useState(
    record?.contactDetails ?? "",
  );
  const [company, setCompany] = React.useState(record?.companyName ?? "");
  const [stage, setStage] = React.useState<CrmStage>(record?.stage ?? "new");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const lock = React.useRef(false);
  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (lock.current) return;
    if (!legal || !unit) {
      setError("请选择法人主体和经营主体");
      return;
    }
    const form = new FormData(event.currentTarget);
    lock.current = true;
    setBusy(true);
    draft.setBusy(true);
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
            accountId: account?.id ?? null,
            contactId: contact?.id ?? null,
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
      void rememberSyncedRecentOperatingUnit("crm-opportunity", unit);
      draft.saved();
      onSaved(result.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : "保存失败，请重试");
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
        <div className="crm-heading">
          <h2>{record ? "编辑商机" : "新建商机"}</h2>
          <button
            type="button"
            onClick={() => {
              draft.discard(onCancel);
            }}
            disabled={busy}
          >
            取消
          </button>
        </div>
        {error && (
          <p className="crm-error" role="alert">
            {error}
          </p>
        )}
        <div className="crm-fields">
          <CrmAccountPicker
            value={account}
            onChange={(value) => {
              setAccount(value);
              setCustomer(value?.customerId ?? "");
              setCompany(value?.name ?? "");
              setContact(null);
              setContactName("");
              setContactDetails("");
              draft.markDirty();
            }}
          />
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
          <OperatingUnitPicker
            label="经营主体"
            records={units}
            value={unit}
            onChange={(value) => {
              setUnit(value);
              draft.markDirty();
            }}
            disabled={Boolean(record)}
            preferenceContext={record ? undefined : "crm-opportunity"}
            preferenceFallback={units.length === 1 ? units[0].id : ""}
          />
          {!account && (
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
          )}
          <label>
            客户公司
            <input
              value={company}
              readOnly={Boolean(account)}
              onChange={(e) => setCompany(e.target.value)}
              required
              maxLength={160}
              placeholder="公司名称"
            />
          </label>
          {account && (
            <CrmContactPicker
              key={account.id}
              accountId={account.id}
              value={contact}
              onChange={(value) => {
                setContact(value);
                setContactName(value?.contactName ?? "");
                setContactDetails(value?.contactDetails ?? "");
                draft.markDirty();
              }}
            />
          )}
          <label>
            联系人
            <input
              name="contactName"
              maxLength={100}
              value={contactName}
              onChange={(e) => setContactName(e.target.value)}
              readOnly={Boolean(contact)}
            />
          </label>
          <label>
            联系方式
            <input
              name="contactDetails"
              maxLength={200}
              value={contactDetails}
              onChange={(e) => setContactDetails(e.target.value)}
              readOnly={Boolean(contact)}
              placeholder="电话、微信或邮箱"
            />
          </label>
          {contact && (
            <p className="crm-hint crm-wide">
              联系人资料统一在“客户联系人”页面维护。
            </p>
          )}
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
      </fieldset>
    </form>
  );
}
