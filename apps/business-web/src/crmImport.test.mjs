import assert from "node:assert/strict";
import test from "node:test";
import { parseTable, importRows, importKey } from "./crmImportData.ts";
test("CSV quotes, BOM, CRLF and spreadsheet paste", () => {
  assert.deepEqual(
    parseTable('\uFEFF商机名称,客户公司\r\n"年度,采购","客户""甲"\r\n'),
    [
      ["商机名称", "客户公司"],
      ["年度,采购", '客户"甲'],
    ],
  );
  assert.deepEqual(parseTable("商机名称\t客户公司\n年度\t客户"), [
    ["商机名称", "客户公司"],
    ["年度", "客户"],
  ]);
  assert.throws(() => parseTable('"bad'), /引号/);
});
test("validate dates, duplicates, currency, conversion and precise money", () => {
  const rows = importRows(
    "商机名称,客户公司,预计金额,跟进日期\n项目,客户,12.34,2026-10-02\n项目,客户,12.34,2026-10-02\n坏日期,客户,1,2026-02-30",
    "le",
    "bu",
  );
  assert.equal(rows[0].payload.expectedAmountMinor, 1234);
  assert.match(rows[1].error, /重复/);
  assert.match(rows[2].error, /日期/);
  assert.match(
    importRows("商机名称,客户公司,阶段\n项目,客户,已成交", "le", "bu")[0].error,
    /正式客户/,
  );
  assert.throws(
    () =>
      importRows(
        "商机名称,客户公司\n" + Array(201).fill("项目,客户").join("\n"),
        "le",
        "bu",
      ),
    /200/,
  );
  assert.throws(
    () => importRows("商机名称,客户公司,未知\n项目,客户,x", "le", "bu"),
    /表头/,
  );
});
test("identical import retains idempotency identity; changed scope does not", async () => {
  const a = importRows("商机名称,客户公司\n项目,客户", "le", "bu")[0].payload;
  assert.equal(await importKey(a), await importKey({ ...a }));
  assert.notEqual(
    await importKey(a),
    await importKey({ ...a, businessUnitId: "other" }),
  );
});
