import { test, expect } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("已成交商机带入订单主体与客户，补充商品后只保存草稿", async ({ page }) => {
  const source = {
    id: "opp",
    title: "年度采购",
    companyName: "客户乙",
    customerId: "customer",
    legalEntityId: "legal",
    businessUnitId: "unit",
    stage: "won",
    currency: "USD",
    contactName: "张经理",
    contactDetails: "13800138000",
    version: 1,
  };
  const writes: any[] = [];
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      path = new URL(req.url()).pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf" },
      });
    if (path === "/api/v1/crm/opportunities/opp")
      return route.fulfill({
        json: { item: source, followups: [], hasOlderFollowups: false },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: { items: [source], canManage: true, hasMore: false },
      });
    if (path === "/api/v1/core-master-data")
      return route.fulfill({
        json: {
          items: [
            {
              id: "unit",
              name: "经营单元",
              code: "OU",
              resourceType: "business_unit",
              status: "active",
              ancestorPath: ["经营单元"],
              parentBusinessUnitId: null,
            },
          ],
        },
      });
    if (path.startsWith("/api/v1/master-data/")) {
      const resource = path.split("/").at(-1)!;
      const ids: Record<string, string> = {
        legal_entity: "legal",
        customer: "customer",
        sku: "sku",
        warehouse: "warehouse",
        unit_of_measure: "uom",
      };
      return route.fulfill({
        json: {
          items: [
            {
              id: ids[resource],
              name: resource,
              code: resource,
              resourceType: resource,
              status: "active",
              legalEntityId: null,
            },
          ],
        },
      });
    }
    if (path === "/api/v1/sales-orders" && req.method() === "GET") {
      expect(new URL(req.url()).searchParams.get("opportunityId")).toBe("opp");
      return route.fulfill({
        json: {
          items: writes.length
            ? [
                {
                  id: "sales-order",
                  orderNumber: "SO-TEST",
                  orderDate: "2026-10-02",
                  currency: "USD",
                  grossAmount: "10",
                  lifecycleStatus: "draft",
                },
              ]
            : [],
        },
      });
    }
    if (req.method() === "POST") {
      expect(path).toBe("/api/v1/sales-orders");
      expect(req.headers()["x-csrf-token"]).toBe("csrf");
      writes.push(req.postDataJSON());
      return route.fulfill({ json: { number: "SO-TEST" } });
    }
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crm?opportunity=opp");
  const drawer = page.getByRole("dialog");
  await drawer.getByRole("button", { name: "创建销售订单草稿" }).click();
  await expect(
    drawer.getByRole("heading", { name: "录入销售订单" }),
  ).toBeVisible();
  await expect(drawer.getByLabel("法律主体", { exact: true })).toHaveValue(
    "legal",
  );
  await expect(drawer.getByLabel("客户", { exact: true })).toHaveValue(
    "customer",
  );
  await expect(drawer.getByLabel("客户参考号", { exact: true })).toHaveValue(
    "CRM:opp",
  );
  expect(
    await drawer
      .locator(".entry-line")
      .evaluate((el) => el.scrollWidth <= el.clientWidth),
  ).toBe(true);
  await expect(
    drawer.getByLabel("客户参考号", { exact: true }),
  ).toHaveAttribute("readonly", "");
  await drawer.getByLabel("第 1 行数量", { exact: true }).fill("2");
  await drawer.getByLabel("第 1 行数量", { exact: true }).fill("1");
  await drawer.getByRole("button", { name: "返回商机" }).click();
  const discard = page.getByRole("dialog", { name: "放弃未保存修改" });
  await expect(discard).toBeVisible();
  await discard.getByRole("button", { name: "继续编辑" }).click();
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-sales-draft.png" });
  await drawer.getByRole("button", { name: "保存销售订单草稿" }).click();
  expect(writes).toHaveLength(0);
  await drawer.getByLabel("第 1 行商品", { exact: true }).selectOption("sku");
  await drawer
    .getByLabel("第 1 行仓库", { exact: true })
    .selectOption("warehouse");
  await drawer.getByLabel("第 1 行单位", { exact: true }).selectOption("uom");
  await drawer.getByLabel("第 1 行单价", { exact: true }).fill("10");
  await drawer.getByRole("button", { name: "保存销售订单草稿" }).click();
  await expect(
    drawer.getByText("销售订单草稿已保存，可前往", { exact: false }),
  ).toBeVisible();
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({
    legalEntityId: "legal",
    customerId: "customer",
    businessUnitId: "unit",
    currency: "USD",
    customerReference: "CRM:opp",
    lines: [{ skuId: "sku", quantity: "1", unitPrice: "10" }],
  });
  await expect(drawer.getByRole("link", { name: /SO-TEST/ })).toHaveAttribute(
    "href",
    "/sales/orders/sales-order",
  );
  source.stage = "lost";
  await drawer.getByRole("button", { name: "创建销售订单草稿" }).click();
  await expect(drawer.getByRole("alert")).toContainText("商机须已成交");
  expect(writes).toHaveLength(1);
});

test("关联订单展示状态、金额与正确链接，读取失败可重试", async ({ page }) => {
  let reads = 0;
  const source = {
    id: "opp",
    title: "年度采购",
    companyName: "客户乙",
    customerId: "customer",
    stage: "won",
    currency: "CNY",
    version: 1,
  };
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session")
      return route.fulfill({ json: { authenticated: true } });
    if (path === "/api/v1/crm/opportunities/opp")
      return route.fulfill({
        json: { item: source, followups: [], hasOlderFollowups: false },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: { items: [source], canManage: false, hasMore: false },
      });
    if (path === "/api/v1/sales-orders") {
      expect(
        new URL(route.request().url()).searchParams.get("opportunityId"),
      ).toBe("opp");
      reads++;
      if (reads === 1)
        return route.fulfill({
          status: 503,
          json: { message: "暂时无法读取订单" },
        });
      return route.fulfill({
        json: {
          items: [
            {
              id: "order",
              orderNumber: "SO-LINKED",
              currency: "CNY",
              grossAmount: "1250",
              lifecycleStatus: "completed",
              orderDate: "2026-10-02",
            },
          ],
        },
      });
    }
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crm?opportunity=opp");
  const drawer = page.getByRole("dialog");
  await expect(drawer.getByRole("alert")).toContainText("暂时无法读取订单");
  await drawer.getByRole("button", { name: "重新加载订单" }).click();
  const link = drawer.getByRole("link", { name: /SO-LINKED/ });
  await expect(link).toHaveAttribute("href", "/sales/orders/order");
  await expect(link).toContainText("已完成");
  await expect(link).toContainText("1,250");
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-related-orders.png" });
  expect(reads).toBe(2);
});
