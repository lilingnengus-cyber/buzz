import React from "react";
import { request } from "./api";
import { CrmDrawer, useCrmDraft } from "./CrmDrawer";
import { OperatingUnitPicker } from "./OperatingUnitPicker";
import { IMPORT_HEADERS, importRows, importKey } from "./crmImportData";
import { CRM_STAGES, type CrmOption, type CrmStage } from "./crm";
import { formatMoney } from "./formatters";
type Row = ReturnType<typeof importRows>[number] & {
  status?: string;
  id?: string;
  key?: string;
};
function download(text: string, name: string) {
  const url = URL.createObjectURL(
    new Blob(["\uFEFF", text], { type: "text/csv;charset=utf-8" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function CrmImport({
  options,
  onClose,
  onChanged,
}: {
  options: CrmOption[];
  onClose: () => void;
  onChanged: () => void;
}) {
  return (
    <CrmDrawer title="批量导入商机" onClose={onClose}>
      <ImportForm options={options} onChanged={onChanged} />
    </CrmDrawer>
  );
}
function ImportForm({
  options,
  onChanged,
}: {
  options: CrmOption[];
  onChanged: () => void;
}) {
  const draft = useCrmDraft();
  const entities = options.filter(
      (o) => o.resourceType === "legal_entity" && o.status === "active",
    ),
    units = options.filter(
      (o) => o.resourceType === "business_unit" && o.status === "active",
    );
  const [legal, setLegal] = React.useState(
    entities.length === 1 ? entities[0].id : "",
  );
  const [unit, setUnit] = React.useState(units.length === 1 ? units[0].id : "");
  const [source, setSource] = React.useState("");
  const [rows, setRows] = React.useState<Row[]>([]);
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [started, setStarted] = React.useState(false);
  const lock = React.useRef(false);
  const invalidate = () => {
    setRows([]);
    setError("");
    draft.markDirty();
  };
  const preview = () => {
    try {
      setRows(importRows(source, legal, unit));
      setError("");
    } catch (e) {
      setRows([]);
      setError(e instanceof Error ? e.message : "解析失败");
    }
  };
  const run = async () => {
    if (lock.current || !rows.length || rows.some((r) => r.error)) return;
    lock.current = true;
    setBusy(true);
    draft.setBusy(true);
    setStarted(true);
    setError("");
    const result = rows.map((r) => ({ ...r }));
    try {
      for (const row of result) {
        if (row.id || !row.payload) continue;
        row.status = "导入中";
        setRows(result.map((r) => ({ ...r })));
        try {
          row.key ||= await importKey(row.payload);
          const saved = await request<{ id: string }>(
            "/api/v1/crm/opportunities",
            {
              method: "POST",
              headers: { "idempotency-key": row.key },
              body: JSON.stringify(row.payload),
            },
          );
          row.id = saved.id;
          row.status = "已保存（重复提交不会新建）";
        } catch (e) {
          row.status = e instanceof Error ? e.message : "导入失败，请重试";
        }
        setRows(result.map((r) => ({ ...r })));
      }
      if (result.every((r) => r.id)) draft.saved();
      onChanged();
    } finally {
      lock.current = false;
      setBusy(false);
      draft.setBusy(false);
    }
  };
  return (
    <div className="crm-form">
      <p>
        支持 UTF-8 CSV 文件或粘贴 Excel 表格，含表头，每批最多 200 条、1
        MB。商机名称和客户公司必填。
      </p>
      <p className="crm-hint">
        统一使用下方主体，负责人为当前账号。客户按潜在客户录入并复用同账号的同名档案，不按名称自动关联正式客户。只新增商机，不覆盖已有记录；相同主体和全部字段完全相同的导入会复用此前结果。
      </p>
      <button
        disabled={busy}
        onClick={() =>
          download(IMPORT_HEADERS.join(",") + "\r\n", "商机导入模板.csv")
        }
      >
        下载 CSV 模板
      </button>
      <fieldset className="crm-edit-fields" disabled={busy || started}>
        <label>
          法人主体
          <select
            value={legal}
            onChange={(e) => {
              setLegal(e.target.value);
              invalidate();
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
          onChange={(v) => {
            setUnit(v);
            invalidate();
          }}
          disabled={busy || started}
        />
        <label>
          上传 CSV
          <input
            type="file"
            accept=".csv,text/csv"
            onChange={async (e) => {
              const file = e.target.files?.[0];
              if (!file) return;
              invalidate();
              if (file.size > 1024 * 1024) {
                setSource("");
                setError("文件不能超过 1 MB");
                return;
              }
              setBusy(true);
              draft.setBusy(true);
              try {
                const value = new TextDecoder("utf-8", { fatal: true }).decode(
                  await file.arrayBuffer(),
                );
                setSource(value);
              } catch {
                setSource("");
                setError("无法读取，请保存为 UTF-8 CSV 后重试");
              } finally {
                setBusy(false);
                draft.setBusy(false);
              }
            }}
          />
        </label>
        <label>
          表格内容
          <textarea
            rows={7}
            value={source}
            maxLength={1024 * 1024}
            placeholder={IMPORT_HEADERS.join("\t")}
            onChange={(e) => {
              setSource(e.target.value);
              invalidate();
            }}
          />
        </label>
        <button onClick={preview}>预览校验</button>
      </fieldset>
      {error && (
        <p role="alert" className="crm-error">
          {error}
        </p>
      )}
      {rows.length > 0 && (
        <>
          <p role="status">
            共 {rows.length} 条，校验错误 {rows.filter((r) => r.error).length}{" "}
            条，已保存 {rows.filter((r) => r.id).length} 条。
          </p>
          <div style={{ overflowX: "auto" }}>
            <table>
              <thead>
                <tr>
                  <th>行</th>
                  <th>商机 / 客户</th>
                  <th>阶段 / 金额</th>
                  <th>预计成交日期 / 流失原因</th>
                  <th>校验及结果</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.row}>
                    <td>{r.row}</td>
                    <td>
                      {r.title}
                      <br />
                      {r.payload?.companyName}
                      <br />
                      {r.payload?.contactName}
                      <br />
                      {r.payload?.contactDetails}
                    </td>
                    <td>
                      {r.payload && CRM_STAGES[r.payload.stage as CrmStage]}
                      <br />
                      {r.payload?.expectedAmountMinor == null
                        ? "未填写"
                        : formatMoney(
                            r.payload.currency,
                            r.payload.expectedAmountMinor / 100,
                          )}
                    </td>
                    <td>
                      预计成交：{r.payload?.expectedCloseDate || "未安排"}
                      <br />
                      {r.payload?.lossReason}
                    </td>
                    <td>
                      {r.error || r.status || "待导入"}
                      {r.id && (
                        <a
                          href={`/#crm?opportunity=${encodeURIComponent(r.id)}`}
                        >
                          查看商机
                        </a>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {rows.some((r) => !r.id) && (
            <button
              className="primary"
              disabled={busy || rows.some((r) => r.error)}
              onClick={run}
            >
              {busy ? "正在导入…" : started ? "重试未成功记录" : "确认导入"}
            </button>
          )}
          {started && (
            <p className="crm-hint">
              逐条保存，成功记录不会回滚。失败时可直接重试；需更正内容时关闭后重新导入，已成功记录保持不变。
            </p>
          )}
        </>
      )}
    </div>
  );
}
