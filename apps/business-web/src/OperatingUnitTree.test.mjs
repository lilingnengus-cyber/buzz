import assert from "node:assert/strict";
import test from "node:test";
import {
  buildOperatingTree,
  updateIndependentSelection,
} from "./OperatingUnitTree.ts";

const unit = (
  id,
  code,
  parentBusinessUnitId,
  name = code,
  status = "active",
) => ({
  id,
  code,
  name,
  status,
  resourceType: "business_unit",
  parentBusinessUnitId,
  ancestorPath: [],
  depth: 0,
  descendantCount: 0,
});

test("builds a sorted four-level tree from unordered records", () => {
  const tree = buildOperatingTree([
    unit("hz", "HZ", "east", "杭州团队"),
    unit("root", "ROOT", null, "集团经营体"),
    unit("east", "EAST", "china", "华东事业部"),
    unit("china", "CHINA", "root", "中国区"),
    unit("bj", "BJ", "china", "北京团队", "disabled"),
  ]);
  assert.equal(tree[0].id, "root");
  assert.equal(tree[0].children[0].id, "china");
  assert.deepEqual(
    tree[0].children[0].children.map((item) => item.id),
    ["bj", "east"],
  );
  assert.equal(tree[0].children[0].children[0].status, "disabled");
  assert.equal(tree[0].children[0].children[1].children[0].id, "hz");
});

test("search retains ancestors and malformed parents stay visible", () => {
  const records = [
    unit("root", "ROOT", null, "集团经营体"),
    unit("east", "EAST", "root", "华东事业部"),
    unit("hz", "HZ", "east", "杭州团队"),
    unit("orphan", "ORPHAN", "missing", "待整理团队"),
  ];
  const filtered = buildOperatingTree(records, "杭州");
  assert.equal(filtered[0].id, "root");
  assert.equal(filtered[0].children[0].children[0].id, "hz");
  const all = buildOperatingTree(records);
  assert.equal(all.at(-1).id, "__orphans__");
  assert.equal(all.at(-1).children[0].id, "orphan");
});

test("legal entity and operating unit selections remain independent", () => {
  const initial = {
    legalEntityId: "legal-cn-id",
    businessUnitId: "hangzhou-id",
  };
  const nextAfterLegalChange = updateIndependentSelection(
    initial,
    "legalEntityId",
    "legal-hk-id",
  );
  assert.equal(nextAfterLegalChange.businessUnitId, "hangzhou-id");
  const nextAfterUnitChange = updateIndependentSelection(
    initial,
    "businessUnitId",
    "beijing-id",
  );
  assert.equal(nextAfterUnitChange.legalEntityId, "legal-cn-id");
});
