import { expect, type Page, test } from "@playwright/test";

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

async function expectScrollableRegisterWithStickyHeaderAndActions(
  page: Page,
  selector: string,
) {
  await page.setViewportSize({ width: 900, height: 768 });
  const register = page.locator(selector);
  await expect
    .poll(() =>
      register.evaluate((element) => element.scrollWidth > element.clientWidth),
    )
    .toBe(true);
  const actions = register.locator("article .master-actions").first();
  const header = register.locator(".master-register-head");
  const registerBox = await register.boundingBox();
  expect(registerBox).not.toBeNull();
  await register.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  await expect
    .poll(async () => {
      const actionBox = await actions.boundingBox();
      if (!actionBox || !registerBox) return false;
      return (
        actionBox.x >= registerBox.x &&
        actionBox.x + actionBox.width <= registerBox.x + registerBox.width + 1
      );
    })
    .toBe(true);
  await register.evaluate((element) => {
    element.scrollLeft = 0;
  });
  await expect
    .poll(() =>
      register.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    )
    .toBe(true);
  await register.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(async () => {
      const headerBox = await header.boundingBox();
      if (!headerBox || !registerBox) return false;
      return Math.abs(headerBox.y - registerBox.y) <= 1;
    })
    .toBe(true);
}

test("客户是集团共享主数据且不选择法定主体或经营主体", async ({ page }) => {
  const legal = record("legal", "LE-0001", "示例法人", "legal_entity");
  const customer = {
    ...legal,
    id: "customer",
    code: "CU-000001",
    name: "示例客户",
    resourceType: "customer",
    legalEntityId: null,
    legalEntityCode: null,
    legalEntityName: null,
    businessUnitId: null,
    businessUnitCode: null,
    businessUnitName: null,
    creditCurrency: "CNY",
    creditLimitMinor: 0,
    paymentTermsDays: 30,
  };
  const items = [
    legal,
    ...Array.from({ length: 10 }, (_, index) =>
      record(
        `legal-${index + 2}`,
        `LE-${String(index + 2).padStart(4, "0")}`,
        `示例法人 ${index + 2}`,
        "legal_entity",
      ),
    ),
    record("group", "OU-0001", "集团", "business_unit", null, ["集团"]),
    customer,
  ];
  const submissions: Array<Record<string, unknown>> = [];
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "group-shared-customer-test",
          displayName: "集团共享客户验收",
          csrfToken: "group-shared-customer-csrf",
        },
      });
    } else if (path === "/api/v1/core-master-data") {
      if (route.request().method() === "POST") {
        submissions.push(
          route.request().postDataJSON() as Record<string, unknown>,
        );
        await route.fulfill({
          json: {
            id: "customer-new",
            resourceType: "customer",
            code: "CU-000002",
            status: "active",
            version: 1,
            traceId: "customer-create-trace",
            idempotentReplay: false,
          },
        });
      } else {
        await route.fulfill({
          json: { items, canManage: true, dataAsOf: "2026-10-01T10:00:00Z" },
        });
      }
    } else if (path === "/api/v1/core-master-data/customer/customer") {
      submissions.push(
        route.request().postDataJSON() as Record<string, unknown>,
      );
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
    } else {
      await route.fulfill({ json: { items: [] } });
    }
  });

  await page.goto("/#coreData");
  const header = page.locator(".master-register-head");
  await expect(header.getByText("编码", { exact: true })).toBeVisible();
  await expect(header.getByText("名称", { exact: true })).toBeVisible();
  await expect(header.getByText("权威关系", { exact: true })).toHaveCount(0);
  await expectScrollableRegisterWithStickyHeaderAndActions(
    page,
    ".master-register",
  );

  await page.getByRole("tab", { name: /客户/ }).click();
  await page.getByRole("button", { name: "＋ 新增客户" }).click();
  const createDialog = page.getByRole("dialog", { name: "新增客户" });
  await expect(createDialog.getByLabel("法定主体 *")).toHaveCount(0);
  await expect(
    createDialog.getByRole("tree", { name: "经营主体 *" }),
  ).toHaveCount(0);
  await expect(createDialog.getByText("集团共享主数据")).toBeVisible();
  await createDialog.getByLabel("名称 *").fill("集团共享测试客户");
  await createDialog.getByRole("button", { name: "确认新增" }).click();
  await expect.poll(() => submissions[0]?.legalEntityId).toBe(null);
  await expect.poll(() => submissions[0]?.businessUnitId).toBe(null);

  await page
    .locator("article")
    .filter({ hasText: "CU-000001" })
    .getByRole("button", { name: "编辑" })
    .click();
  const editDialog = page.getByRole("dialog", { name: "编辑客户" });
  await expect(editDialog.getByLabel("法定主体 *")).toHaveCount(0);
  await expect(
    editDialog.getByRole("tree", { name: "经营主体 *" }),
  ).toHaveCount(0);
  await editDialog.getByRole("button", { name: "保存修订" }).click();
  await expect.poll(() => submissions[1]?.legalEntityId).toBe(null);
  await expect.poll(() => submissions[1]?.businessUnitId).toBe(null);
});

test("商品主数据编码与名称独立展示", async ({ page }) => {
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") {
      await route.fulfill({
        json: {
          authenticated: true,
          subject: "product-register-test",
          displayName: "商品列表验收",
          csrfToken: "product-register-csrf",
        },
      });
    } else if (path === "/api/v1/product-master-data") {
      await route.fulfill({
        json: {
          items: Array.from({ length: 12 }, (_, index) => ({
            resourceType: "product",
            id: `product-${index + 1}`,
            code: `SPU-${String(index + 1).padStart(4, "0")}`,
            name: `示例商品 ${index + 1}`,
            status: "active",
            categoryId: "category-1",
            categoryCode: "CAT-0001",
            categoryName: "示例分类",
            brandId: "brand-1",
            brandCode: "BRD-0001",
            brandName: "示例品牌",
            unitOfMeasureId: "uom-1",
            unitOfMeasureCode: "PCS",
            unitOfMeasureName: "件",
            allowZeroCost: false,
            version: 1,
            updatedAt: "2026-10-01T09:00:00Z",
          })),
          canManage: true,
          dataAsOf: "2026-10-01T09:00:00Z",
        },
      });
    } else {
      await route.fulfill({ json: { items: [] } });
    }
  });

  await page.goto("/#productData");
  const header = page.locator(".product-register .master-register-head");
  await expect(header.locator("span")).toHaveText([
    "编码",
    "名称",
    "商品关系",
    "识别与计量",
    "状态 / 版本",
    "操作",
  ]);
  const row = page.locator(".product-register article");
  await expect(row.first().locator(".master-code")).toHaveText("SPU-0001");
  await expect(row.first().locator(".master-name strong")).toHaveText(
    "示例商品 1",
  );
  await expectScrollableRegisterWithStickyHeaderAndActions(
    page,
    ".product-register",
  );
});
