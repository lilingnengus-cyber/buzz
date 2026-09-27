import { expect, test } from "@playwright/test";

const record = (
  id: string,
  code: string,
  name: string,
  resourceType: "legal_entity" | "business_unit",
  parentBusinessUnitId: string | null = null,
  ancestorPath: string[] = [],
) => ({
  id,
  code,
  name,
  resourceType,
  status: "active",
  legalEntityId: resourceType === "legal_entity" ? null : "legal",
  legalEntityCode: resourceType === "legal_entity" ? null : "LE-0001",
  legalEntityName: resourceType === "legal_entity" ? null : "示例法人",
  businessUnitId: null,
  businessUnitCode: null,
  businessUnitName: null,
  parentBusinessUnitId,
  ancestorPath,
  depth: Math.max(0, ancestorPath.length - 1),
  descendantCount: 0,
  countryCode: null,
  functionalCurrency: null,
  registrationNumber: null,
  address: null,
  creditCurrency: null,
  creditLimitMinor: null,
  paymentTermsDays: null,
  version: 1,
  updatedAt: "2026-09-27T10:00:00Z",
});

test("新增客户通过经营组织树选择经营主体", async ({ page }) => {
  const items = [
    record("legal", "LE-0001", "示例法人", "legal_entity"),
    record("group", "OU-0001", "集团", "business_unit", null, ["集团"]),
    record("china", "OU-0002", "中国区", "business_unit", "group", [
      "集团",
      "中国区",
    ]),
    record("east", "OU-0003", "华东区", "business_unit", "china", [
      "集团",
      "中国区",
      "华东区",
    ]),
    record("hangzhou", "OU-0004", "杭州单元", "business_unit", "east", [
      "集团",
      "中国区",
      "华东区",
      "杭州单元",
    ]),
    record("beijing", "OU-0005", "北京单元", "business_unit", "china", [
      "集团",
      "中国区",
      "北京单元",
    ]),
  ];
  let submitted: Record<string, unknown> | null = null;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "tree-picker-test",
          displayName: "树形选择验收",
          csrfToken: "tree-picker-csrf",
        },
      });
    } else if (path === "/api/v1/core-master-data") {
      if (route.request().method() === "POST") {
        submitted = route.request().postDataJSON() as Record<string, unknown>;
        await route.fulfill({
          json: {
            id: "customer-new",
            resourceType: "customer",
            code: "CU-LE-0001-00001",
            status: "active",
            version: 1,
            traceId: "tree-picker-trace",
            idempotentReplay: false,
          },
        });
      } else {
        await route.fulfill({
          json: {
            items,
            canManage: true,
            dataAsOf: "2026-09-27T10:00:00Z",
          },
        });
      }
    } else {
      await route.fulfill({ json: { items: [] } });
    }
  });

  await page.goto("/#coreData");
  await page.getByRole("tab", { name: /客户/ }).click();
  await page.getByRole("button", { name: "＋ 新增客户" }).click();

  const dialog = page.getByRole("dialog", { name: "新增客户" });
  const tree = dialog.getByRole("tree", { name: "经营主体 *" });
  await expect(tree.getByRole("treeitem")).toHaveCount(5);
  const search = dialog.getByRole("searchbox", { name: "经营主体 *搜索" });
  const group = tree.getByRole("button", { name: /OU-0001.*集团/ });
  await search.press("ArrowDown");
  await expect(group).toBeFocused();
  await group.press("ArrowLeft");
  await expect(tree.getByRole("treeitem")).toHaveCount(1);
  await group.press("ArrowRight");
  await expect(tree.getByRole("treeitem")).toHaveCount(5);
  await search.fill("OU-0004");
  await expect(tree.getByRole("treeitem")).toHaveCount(4);
  await expect(tree.getByText("北京单元")).toHaveCount(0);
  await search.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  const hangzhou = tree.getByRole("button", { name: /OU-0004.*杭州单元/ });
  await expect(hangzhou).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    dialog.getByText("集团 / 中国区 / 华东区 / 杭州单元"),
  ).toBeVisible();

  await dialog.getByLabel("名称 *").fill("树形选择测试客户");
  await dialog.getByLabel("法定主体 *").selectOption("legal");
  await dialog.getByRole("button", { name: "确认新增" }).click();
  await expect.poll(() => submitted?.businessUnitId).toBe("hangzhou");
  await expect(dialog).toHaveCount(0);
  await page.getByRole("button", { name: "＋ 新增客户" }).click();
  await expect(
    page
      .getByRole("dialog", { name: "新增客户" })
      .getByText("集团 / 中国区 / 华东区 / 杭州单元"),
  ).toBeVisible();
});
