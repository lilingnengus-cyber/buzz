export const IMPORT_HEADERS = [
  "线索名称",
  "公司名称",
  "联系人",
  "联系方式",
  "来源",
  "需求摘要",
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
export function importRows(text: string, uniformSource = "") {
  if (new TextEncoder().encode(text).length > 1024 * 1024)
    throw new Error("文件不能超过 1 MB");
  const [rawHeaders, ...rows] = parseTable(text);
  if (!rawHeaders || !rows.length) throw new Error("请提供表头及至少一条线索");
  const headers = rawHeaders.map((h) => (h === "客户公司" ? "公司名称" : h));
  if (rawHeaders.includes("公司名称") && rawHeaders.includes("客户公司"))
    throw new Error("公司名称与客户公司是同一字段，请仅保留一列");
  if (rows.length > 200) throw new Error("每批最多导入 200 条线索");
  if (
    new Set(headers).size !== headers.length ||
    headers.some((h) => !IMPORT_HEADERS.includes(h))
  )
    throw new Error("表头重复或不受支持，请使用线索导入模板");
  if (!headers.includes("线索名称")) throw new Error("缺少线索名称列");
  const seen = new Set<string>();
  return rows.map((cells, index) => {
    const title = cells[headers.indexOf("线索名称")] || "";
    try {
      if (cells.length !== headers.length) throw new Error("列数与表头不一致");
      const get = (name: string) =>
        name === "来源" && uniformSource ? uniformSource : cells[headers.indexOf(name)] || "";
      for (const [name, max] of [
        ["线索名称", 160],
        ["公司名称", 160],
        ["联系人", 100],
        ["联系方式", 200],
        ["来源", 100],
        ["需求摘要", 4000],
      ] as const) {
        if (Array.from(get(name)).length > max)
          throw new Error(`${name}超过 ${max} 字`);
      }
      if (!title) throw new Error("线索名称必填");
      const payload = {
        title,
        companyName: get("公司名称"),
        contactName: get("联系人"),
        contactDetails: get("联系方式"),
        source: get("来源"),
        summary: get("需求摘要"),
        nextAction: "",
        nextFollowUp: null,
        customerId: null,
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
  return `crm-lead-import-v1-${Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("")}`;
}
