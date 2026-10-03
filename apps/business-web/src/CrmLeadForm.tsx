import React from "react";
import { request } from "./api";
import { useCrmCommand } from "./useCrmCommand";
import { useCrmDraft } from "./CrmDrawer";
import { CrmSearchSelect } from "./CrmSearchSelect";
import type { Lead } from "./crmLeads";

export function CrmLeadForm({
  record,
  onSaved,
  onCancel,
}: {
  record?: Lead;
  onSaved: (id: string, transferred: boolean) => void;
  onCancel: () => void;
}) {
  const command = useCrmCommand();
  const draft = useCrmDraft();
  const [busy, setBusy] = React.useState(false);
  const lock = React.useRef(false);
  const [error, setError] = React.useState("");
  const [optionError, setOptionError] = React.useState("");
  const [revision, setRevision] = React.useState(0);
  const [owners, setOwners] = React.useState<{ id: string; name: string }[]>(
    [],
  );
  const [owner, setOwner] = React.useState(record?.ownerUserId ?? "");
  const [ownerQuery, setOwnerQuery] = React.useState("");
  React.useEffect(() => {
    let active = true;
    setOptionError("");
    request<{ items: { id: string; name: string }[] }>(
      "/api/v1/crm/leads/owners",
    )
      .then((people) => {
        if (active) setOwners(people.items);
      })
      .catch((e) => {
        if (active)
          setOptionError(e instanceof Error ? e.message : "选项加载失败");
      });
    return () => {
      active = false;
    };
  }, [revision]);
  return (
    <form
      className="crm-form"
      onChangeCapture={(e) => {
        if (
          !(e.target instanceof HTMLInputElement && e.target.type === "search")
        )
          draft.markDirty();
      }}
      onSubmit={async (e) => {
        e.preventDefault();
        if (lock.current) return;
        const form = new FormData(e.currentTarget);
        lock.current = true;
        setBusy(true);
        draft.setBusy(true);
        setError("");
        try {
          const body = Object.fromEntries(
            [
              "title",
              "companyName",
              "contactName",
              "contactDetails",
              "source",
              "summary",
              "nextAction",
            ].map((k) => [k, String(form.get(k) ?? "")]),
          );
          const result = await command<{ id: string; transferred: boolean }>(
            `/api/v1/crm/leads${record ? `/${record.id}` : ""}`,
            {
              method: record ? "PUT" : "POST",
              body: JSON.stringify({
                ...body,
                ownerUserId: owner || null,
                customerId: record?.customerId ?? null,
                nextFollowUp: form.get("nextFollowUp") || null,
                expectedVersion: record?.version ?? null,
              }),
            },
          );
          draft.saved();
          onSaved(result.id, result.transferred);
        } catch (e) {
          setError(e instanceof Error ? e.message : "保存失败");
        } finally {
          lock.current = false;
          setBusy(false);
          draft.setBusy(false);
        }
      }}
    >
      <fieldset className="crm-edit-fields" disabled={busy}>
        <div className="crm-heading">
          <h2>{record ? "编辑线索" : "新建线索"}</h2>
          <button type="button" onClick={() => draft.discard(onCancel)}>
            取消
          </button>
        </div>
        <p className="crm-hint">
          先记下需求，只有线索名称必填。筛选通过后再转为商机。
        </p>
        {error && (
          <p role="alert" className="crm-error">
            {error}
          </p>
        )}
        {optionError && (
          <p role="alert">
            负责人加载失败：{optionError}{" "}
            <button type="button" onClick={() => setRevision((v) => v + 1)}>
              重试负责人
            </button>
          </p>
        )}
        <div className="crm-fields">
          {(
            [
              ["title", "线索名称", 160],
              ["companyName", "公司名称", 160],
              ["contactName", "联系人", 100],
              ["contactDetails", "联系方式", 200],
              ["source", "线索来源", 100],
              ["nextAction", "下一步", 500],
            ] as const
          ).map(([key, label, max]) => (
            <label key={key}>
              {label}
              <input
                name={key}
                required={key === "title"}
                maxLength={max}
                defaultValue={record?.[key] ?? ""}
              />
            </label>
          ))}
          <label>
            跟进日期
            <input
              type="date"
              name="nextFollowUp"
              defaultValue={record?.nextFollowUp ?? ""}
            />
          </label>
          <CrmSearchSelect
            label="负责人"
            value={owner}
            selectedLabel={
              owner
                ? (owners.find((o) => o.id === owner)?.name ??
                  record?.ownerName)
                : "自己（默认）"
            }
            query={ownerQuery}
            onQuery={setOwnerQuery}
            options={[
              { value: "", label: "自己（默认）" },
              ...owners
                .filter((o) => o.name.includes(ownerQuery))
                .map((o) => ({ value: o.id, label: o.name })),
            ]}
            onChange={(v) => {
              setOwner(v);
              draft.markDirty();
            }}
          />
          <label className="crm-wide">
            需求摘要
            <textarea
              name="summary"
              rows={4}
              maxLength={4000}
              defaultValue={record?.summary ?? ""}
            />
          </label>
        </div>
        <button type="submit" className="primary" disabled={busy}>
          {busy ? "保存中…" : "保存线索"}
        </button>
      </fieldset>
    </form>
  );
}
