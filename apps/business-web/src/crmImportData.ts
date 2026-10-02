import { amountMinor, CRM_STAGES } from "./crm.ts";
export const IMPORT_HEADERS = [
  "商机名称",
  "客户公司",
  "联系人",
  "联系方式",
  "阶段",
  "预计金额",
  "币种",
  "预计成交日期",
  "流失原因",
];
export function parseTable(source: string): string[][] {
  const text = source.replace(/^\uFEFF/, "");
  const separator = text
    .slice(0, text.indexOf("\n") < 0 ? undefined : text.indexOf("\n"))
    .includes("\t")
    ? "\t"
    : ",";
  const rows: string[][] = [];
  let row: string[] = [],
    cell = "",
    quoted = false,
    ended = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i++;
        } else {
          quoted = false;
          ended = true;
        }
      } else cell += c;
    } else if (c === separator || c === "\n" || c === "\r") {
      row.push(cell.trim());
      cell = "";
      ended = false;
      if (c !== separator) {
        if (c === "\r" && text[i + 1] === "\n") i++;
        if (row.some(Boolean)) rows.push(row);
        row = [];
      }
    } else if (c === '"' && !cell && !ended) quoted = true;
    else {
      if (ended || c === '"')
        throw new Error("引号格式错误，请使用模板重新导出 CSV。");
      cell += c;
    }
  }
  if (quoted) throw new Error("存在未闭合的引号。");
  row.push(cell.trim());
  if (row.some(Boolean)) rows.push(row);
  return rows;
}
function date(value: string, label: string) {
  if (!value) return null;
  if (
    !/^\d{4}-\d{2}-\d{2}$/.test(value) ||
    !Number.isFinite(Date.parse(value)) ||
    new Date(value).toISOString().slice(0, 10) !== value
  )
    throw new Error(`${label}须为有效的 YYYY-MM-DD 日期`);
  return value;
}
export function importRows(
  text: string,
  legalEntityId: string,
  businessUnitId: string,
) {
  if (!legalEntityId || !businessUnitId)
    throw new Error("请选择法人主体和经营主体");
  if (new TextEncoder().encode(text).length > 1024 * 1024)
    throw new Error("文件不能超过 1 MB");
  const [headers, ...rows] = parseTable(text);
  if (!headers || !rows.length) throw new Error("请提供表头及至少一条商机");
  if (rows.length > 200) throw new Error("每批最多导入 200 条商机");
  if (
    new Set(headers).size !== headers.length ||
    headers.some((h) => !IMPORT_HEADERS.includes(h))
  )
    throw new Error("表头重复或不受支持，请使用导入模板");
  if (!["商机名称", "客户公司"].every((h) => headers.includes(h)))
    throw new Error("缺少商机名称或客户公司列");
  const seen = new Set<string>();
  return rows.map((cells, index) => {
    const title = cells[headers.indexOf("商机名称")] || "";
    try {
      if (cells.length !== headers.length) throw new Error("列数与表头不一致");
      const get = (name: string) => cells[headers.indexOf(name)] || "";
      const stageLabel = get("阶段") || "新线索";
      const stage = Object.entries(CRM_STAGES).find(
        ([key, label]) => key === stageLabel || label === stageLabel,
      )?.[0];
      if (!stage) throw new Error("未知商机阶段");
      if (stage === "won")
        throw new Error(
          "成交商机请先以沟通中导入，再通过跟进记录确认正式客户资料",
        );
      for (const [name, max] of [
        ["商机名称", 160],
        ["客户公司", 160],
        ["联系人", 100],
        ["联系方式", 200],
        ["流失原因", 1000],
      ] as const)
        if (Array.from(get(name)).length > max)
          throw new Error(`${name}超过 ${max} 字`);
      if (!title || !get("客户公司")) throw new Error("商机名称和客户公司必填");
      if (get("联系方式") && !get("联系人"))
        throw new Error("填写联系方式时须填写联系人");
      if (stage === "lost" && !get("流失原因"))
        throw new Error("已流失商机须填写流失原因");
      const currency = get("币种") || "CNY";
      if (!/^[A-Z]{3}$/.test(currency))
        throw new Error("币种须为三位大写代码，例如 CNY");
      const payload = {
        legalEntityId,
        businessUnitId,
        customerId: null,
        accountId: null,
        contactId: null,
        title,
        companyName: get("客户公司"),
        contactName: get("联系人"),
        contactDetails: get("联系方式"),
        stage,
        expectedAmountMinor: amountMinor(get("预计金额")),
        currency,
        nextAction: "",
        nextFollowUp: null,
        expectedCloseDate: date(get("预计成交日期"), "预计成交日期"),
        lossReason: stage === "lost" ? get("流失原因") : "",
        ownerUserId: null,
        expectedVersion: null,
      };
      const signature = JSON.stringify(payload);
      if (seen.has(signature)) throw new Error("与本批前面记录完全重复");
      seen.add(signature);
      return { row: index + 2, title, payload, error: "" };
    } catch (e) {
      return {
        row: index + 2,
        title,
        payload: null,
        error: e instanceof Error ? e.message : "数据无效",
      };
    }
  });
}
export async function importKey(payload: object) {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(JSON.stringify(payload)),
  );
  return `crm-import-v1-${Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("")}`;
}
