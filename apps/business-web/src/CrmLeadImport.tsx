import React from "react";
import { request } from "./api";
import { CrmDrawer, useCrmDraft } from "./CrmDrawer";
import { IMPORT_HEADERS, importRows, importKey } from "./crmImportData";
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
export function CrmLeadImport({
  onClose,
  onChanged,
}: {
  onClose: () => void;
  onChanged: () => void;
}) {
  return (
    <CrmDrawer title="批量导入线索" onClose={onClose}>
      <ImportForm onChanged={onChanged} />
    </CrmDrawer>
  );
}
function ImportForm({ onChanged }: { onChanged: () => void }) {
  const draft = useCrmDraft();
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
      setRows(importRows(source));
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
          const saved = await request<{ id: string }>("/api/v1/crm/leads", {
            method: "POST",
            headers: { "idempotency-key": row.key },
            body: JSON.stringify(row.payload),
          });
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
        MB。仅线索名称必填。
      </p>
      <p className="crm-hint">
        导入为待筛选线索，负责人为当前账号。不会创建商机或正式客户；筛选后可从线索详情转为商机。只新增、不覆盖已有记录；同一账号全部字段完全相同的导入会复用此前结果。
      </p>
      <button
        disabled={busy}
        onClick={() =>
          download(IMPORT_HEADERS.join(",") + "\r\n", "线索导入模板.csv")
        }
      >
        下载 CSV 模板
      </button>
      <fieldset className="crm-edit-fields" disabled={busy || started}>
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
                  <th>线索名称</th>
                  <th>公司名称</th>
                  <th>联系人</th>
                  <th>联系方式</th>
                  <th>来源</th>
                  <th>需求摘要</th>
                  <th>下一步</th>
                  <th>跟进日期</th>
                  <th>校验及结果</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.row}>
                    <td>{r.row}</td>
                    <td>{r.title}</td>
                    <td>{r.payload?.companyName || "未填写"}</td>
                    <td>{r.payload?.contactName || "未填写"}</td>
                    <td>{r.payload?.contactDetails || "未填写"}</td>
                    <td>{r.payload?.source || "未填写来源"}</td>
                    <td>{r.payload?.summary || "未填写"}</td>
                    <td>{r.payload?.nextAction || "未安排下一步"}</td>
                    <td>{r.payload?.nextFollowUp || "未安排跟进日期"}</td>
                    <td>
                      {r.error || r.status || "待导入"}
                      {r.id && (
                        <a href={`/#crmLeads?lead=${encodeURIComponent(r.id)}`}>
                          查看线索
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
