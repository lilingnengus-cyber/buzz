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
  let serverPreference: string | null = null;
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
    } else if (
      path === "/api/v1/preferences/operating-unit/core-master-customer"
    ) {
      if (route.request().method() === "PUT") {
        serverPreference = (
          route.request().postDataJSON() as {
            businessUnitId: string;
          }
        ).businessUnitId;
      }
      await route.fulfill({
        json: {
          context: "core-master-customer",
          businessUnitId: serverPreference,
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
  const header = page.locator(".master-register-head");
  await expect(header.getByText("编码", { exact: true })).toBeVisible();
  await expect(header.getByText("名称", { exact: true })).toBeVisible();
  await expect(header.getByText("权威关系", { exact: true })).toHaveCount(0);
  await page
    .locator("article")
    .filter({ hasText: "LE-0001" })
    .getByRole("button", { name: "编辑" })
    .click();
  const legalDialog = page.getByRole("dialog", { name: "编辑法定主体" });
  await expect(legalDialog.getByLabel("登记编号")).toHaveCount(0);
  await legalDialog.getByRole("button", { name: "关闭弹窗" }).click();
  await page.getByRole("tab", { name: /客户/ }).click();
  await page.getByRole("button", { name: "＋ 新增客户" }).click();

  const dialog = page.getByRole("dialog", { name: "新增客户" });
  await expect(dialog.getByRole("tree", { name: "经营主体 *" })).toHaveCount(0);
  await expect(
    dialog.getByRole("searchbox", { name: "经营主体 *搜索" }),
  ).toHaveCount(0);
  await dialog.getByRole("button", { name: /当前选择.*尚未选择/ }).click();
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
  await expect.poll(() => serverPreference).toBe("hangzhou");
  await page.evaluate(() =>
    localStorage.removeItem(
      "business.recent-operating-unit.core-master-customer",
    ),
  );
  await page.getByRole("button", { name: "＋ 新增客户" }).click();
  await expect(
    page
      .getByRole("dialog", { name: "新增客户" })
      .getByText("集团 / 中国区 / 华东区 / 杭州单元"),
  ).toBeVisible();
});

test("编辑客户可以重新选择经营主体", async ({ page }) => {
  const legal = record("legal", "LE-0001", "示例法人", "legal_entity");
  const group = record("group", "OU-0001", "集团", "business_unit", null, [
    "集团",
  ]);
  const hangzhou = record(
    "hangzhou",
    "OU-0002",
    "杭州单元",
    "business_unit",
    "group",
    ["集团", "杭州单元"],
  );
  const beijing = record(
    "beijing",
    "OU-0003",
    "北京单元",
    "business_unit",
    "group",
    ["集团", "北京单元"],
  );
  const customer = {
    ...legal,
    id: "customer",
    code: "CU-000001",
    name: "示例客户",
    resourceType: "customer",
    legalEntityId: "legal",
    legalEntityCode: "LE-0001",
    legalEntityName: "示例法人",
    businessUnitId: "hangzhou",
    businessUnitCode: "OU-0002",
    businessUnitName: "杭州单元",
    creditCurrency: "CNY",
    creditLimitMinor: 0,
    paymentTermsDays: 30,
  };
  const items = [legal, group, hangzhou, beijing, customer];
  let submitted: Record<string, unknown> | null = null;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "customer-edit-test",
          displayName: "客户编辑验收",
          csrfToken: "customer-edit-csrf",
        },
      });
    } else if (path === "/api/v1/core-master-data/customer/customer") {
      submitted = route.request().postDataJSON() as Record<string, unknown>;
      await route.fulfill({
        json: {
          id: "customer",
          resourceType: "customer",
          code: "CU-000001",
          status: "active",
          version: 2,
          traceId: "customer-edit-trace",
          idempotentReplay: false,
        },
      });
    } else if (path === "/api/v1/core-master-data") {
      await route.fulfill({
        json: { items, canManage: true, dataAsOf: "2026-09-30T10:00:00Z" },
      });
    } else {
      await route.fulfill({ json: { items: [] } });
    }
  });

  await page.goto("/#coreData");
  await page.getByRole("tab", { name: /客户/ }).click();
  await page
    .locator("article")
    .filter({ hasText: "CU-000001" })
    .getByRole("button", { name: "编辑" })
    .click();

  const dialog = page.getByRole("dialog", { name: "编辑客户" });
  await expect(
    dialog.getByText("可调整经营主体，保存时校验当前版本"),
  ).toBeVisible();
  const selection = dialog.getByRole("button", {
    name: /当前选择.*集团 \/ 杭州单元/,
  });
  await expect(selection).toBeEnabled();
  await selection.click();
  await dialog
    .getByRole("tree", { name: "经营主体 *" })
    .getByRole("button", { name: /OU-0003.*北京单元/ })
    .click();
  await dialog.getByRole("button", { name: "保存修订" }).click();

  await expect.poll(() => submitted?.businessUnitId).toBe("beijing");
  await expect.poll(() => submitted?.legalEntityId).toBe("legal");
  await expect(dialog).toHaveCount(0);
});
