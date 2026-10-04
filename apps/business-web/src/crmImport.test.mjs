import assert from "node:assert/strict";
import test from "node:test";
import {
  IMPORT_HEADERS,
  parseTable,
  importRows,
  importKey,
} from "./crmImportData.ts";
test("company header rename preserves legacy payload and import identity", async () => {
  assert.ok(IMPORT_HEADERS.includes("公司名称"));
  assert.ok(!IMPORT_HEADERS.includes("客户公司"));
  const current = importRows("线索名称,公司名称\n项目,公司")[0].payload;
  const legacy = importRows("线索名称,客户公司\n项目,公司")[0].payload;
  assert.equal(current.companyName, "公司");
  assert.deepEqual(current, legacy);
  assert.equal(await importKey(current), await importKey(legacy));
  assert.throws(
    () => importRows("线索名称,公司名称,客户公司\n项目,甲,乙"),
    /仅保留一列/,
  );
  for (const name of ["公司名称", "客户公司"]) {
    assert.match(
      importRows(`线索名称,${name}\n项目,${"字".repeat(161)}`)[0].error,
      /公司名称超过 160 字/,
    );
  }
});
test("CSV quotes, BOM, CRLF and spreadsheet paste", () => {
  assert.deepEqual(
    parseTable('\uFEFF线索名称,客户公司\r\n"年度,采购","客户""甲"\r\n'),
    [
      ["线索名称", "客户公司"],
      ["年度,采购", '客户"甲'],
    ],
  );
  assert.deepEqual(parseTable("线索名称\t客户公司\n年度\t客户"), [
    ["线索名称", "客户公司"],
    ["年度", "客户"],
  ]);
  assert.throws(() => parseTable('"bad'), /引号/);
});
test("lead import accepts only title, validates optional dates, limits and duplicates", () => {
  const rows = importRows(
    "线索名称,跟进日期\n项目,2026-10-02\n项目,2026-10-02\n坏日期,2026-02-30",
  );
  assert.equal(rows[0].payload.companyName, "");
  assert.equal(rows[0].payload.nextFollowUp, "2026-10-02");
  assert.equal(rows[0].payload.ownerUserId, null);
  assert.equal(rows[0].payload.customerId, null);
  assert.equal(rows[0].payload.expectedVersion, null);
  assert.match(rows[1].error, /重复/);
  assert.match(rows[2].error, /日期/);
  assert.match(importRows("线索名称\n" + "字".repeat(161))[0].error, /160/);
  assert.match(
    importRows("线索名称,需求摘要\n项目," + "字".repeat(4001))[0].error,
    /4000/,
  );
  assert.throws(
    () => importRows("线索名称\n" + Array(201).fill("项目").join("\n")),
    /200/,
  );
  assert.throws(() => importRows("线索名称,阶段\n项目,已成交"), /表头/);
  assert.throws(() => importRows("商机名称,客户公司\n项目,客户"), /表头/);
  assert.throws(() => importRows("线索名称\n" + "字".repeat(400000)), /1 MB/);
});
test("identical lead import retains its identity; changed content does not", async () => {
  const a = importRows("线索名称,客户公司\n项目,客户")[0].payload;
  assert.equal(await importKey(a), await importKey({ ...a }));
  assert.match(await importKey(a), /^crm-lead-import-v1-/);
  assert.notEqual(
    await importKey(a),
    await importKey({ ...a, nextAction: "联系客户" }),
  );
});
