import { expect, test } from "@playwright/test";

const contexts = [
  "sales-order",
  "purchase-order",
  "crm-opportunity",
  "core-master-customer",
  "core-master-supplier",
  "core-master-warehouse",
];

const units = [
  {
    id: "group",
    code: "OU-0001",
    name: "集团",
    resourceType: "business_unit",
    status: "active",
    parentBusinessUnitId: null,
    ancestorPath: ["集团"],
    depth: 0,
    descendantCount: 2,
  },
  {
    id: "east",
    code: "OU-0002",
    name: "华东区",
    resourceType: "business_unit",
    status: "active",
    parentBusinessUnitId: "group",
    ancestorPath: ["集团", "华东区"],
    depth: 1,
    descendantCount: 1,
  },
  {
    id: "hangzhou",
    code: "OU-0003",
    name: "杭州经营单元",
    resourceType: "business_unit",
    status: "active",
    parentBusinessUnitId: "east",
    ancestorPath: ["集团", "华东区", "杭州经营单元"],
    depth: 2,
    descendantCount: 0,
  },
];

test("用户可以固定并清除各业务场景的默认经营主体", async ({ page }) => {
  const preferences = new Map(
    contexts.map((context) => [
      context,
      { context, businessUnitId: null as string | null, pinned: false },
    ]),
  );
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "preference-test",
          displayName: "设置验收",
          csrfToken: "preference-csrf",
        },
      });
      return;
    }
    if (path === "/api/v1/core-master-data") {
      await route.fulfill({
        json: {
          items: units,
          canManage: true,
          dataAsOf: "2026-09-28T10:00:00Z",
        },
      });
      return;
    }
    const match = path.match(/^\/api\/v1\/preferences\/operating-unit\/(.+)$/);
    if (match) {
      const context = decodeURIComponent(match[1]);
      const method = route.request().method();
      if (method === "PUT") {
        const input = route.request().postDataJSON() as {
          businessUnitId: string;
          pinned: boolean;
        };
        preferences.set(context, { context, ...input });
      } else if (method === "DELETE") {
        preferences.set(context, {
          context,
          businessUnitId: null,
          pinned: false,
        });
      }
      await route.fulfill({ json: preferences.get(context) });
      return;
    }
    await route.fulfill({ json: { items: [] } });
  });

  await page.goto("/#preferences");
  await expect(
    page.getByRole("heading", { name: "默认经营主体" }),
  ).toBeVisible();
  await page.getByRole("button", { name: /采购订单/ }).click();
  await page.getByRole("button", { name: /当前选择.*尚未选择/ }).click();
  await page
    .getByRole("tree", { name: "采购订单默认经营主体" })
    .getByRole("button", { name: /OU-0003.*杭州经营单元/ })
    .click();
  await page.getByRole("button", { name: "固定为默认值" }).click();

  await expect
    .poll(() => preferences.get("purchase-order"))
    .toEqual({
      context: "purchase-order",
      businessUnitId: "hangzhou",
      pinned: true,
    });
  await expect(page.getByText("采购订单的默认经营主体已固定")).toBeVisible();
  await expect(
    page.getByRole("button", { name: /采购订单.*已固定/ }),
  ).toBeVisible();

  await page.reload();
  await page.getByRole("button", { name: /采购订单.*已固定/ }).click();
  await expect(
    page.getByText("集团 / 华东区 / 杭州经营单元").first(),
  ).toBeVisible();
  await page.getByRole("button", { name: "清除设置" }).click();
  await expect
    .poll(() => preferences.get("purchase-order"))
    .toEqual({
      context: "purchase-order",
      businessUnitId: null,
      pinned: false,
    });
  await expect(page.getByText("采购订单已恢复使用业务默认值")).toBeVisible();
  await expect(
    page.getByRole("button", { name: /采购订单.*使用业务默认/ }),
  ).toBeVisible();
});
