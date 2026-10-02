import { test, expect } from "@playwright/test";

test("成交确认正式客户和联系人，失败保留输入，成功刷新档案", async ({
  page,
}) => {
  let item: any = {
    id: "opp",
    title: "年度采购",
    companyName: "杭州客户",
    contactName: "张经理",
    contactDetails: "",
    customerId: null,
    stage: "quoting",
    currency: "CNY",
    nextAction: "签约",
    nextFollowUp: null,
    version: 1,
  };
  let attempts = 0;
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      path = new URL(req.url()).pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf" },
      });
    if (path === "/api/v1/crm/options")
      return route.fulfill({
        json: {
          items: [
            {
              id: "customer",
              name: "已有客户",
              code: "CU-1",
              resourceType: "customer",
              status: "active",
            },
          ],
        },
      });
    if (path.endsWith("/convert-customer")) {
      expect(req.headers()["x-csrf-token"]).toBe("csrf");
      expect(req.headers()["idempotency-key"]).toBeTruthy();
      const body = req.postDataJSON();
      expect(body.expectedVersion).toBe(1);
      expect(body.contactDetails).toBe("13800138000");
      expect(body.customerId).toBe("customer");
      attempts++;
      if (attempts === 1)
        return route.fulfill({
          status: 409,
          json: { message: "记录已更新，请核对重试" },
        });
      item = {
        ...item,
        customerId: "customer",
        companyName: "已有客户",
        stage: "won",
        contactDetails: body.contactDetails,
        version: 2,
      };
      return route.fulfill({
        json: {
          id: "opp",
          customerId: "customer",
          contactId: "contact",
          version: 2,
        },
      });
    }
    if (path === "/api/v1/crm/opportunities/opp")
      return route.fulfill({
        json: { item, followups: [], hasOlderFollowups: false },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: { items: [item], hasMore: false, canManage: true },
      });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crmFollowups");
  await page.getByRole("button", { name: "新建跟进", exact: true }).click();
  const drawer = page.getByRole("dialog");
  await drawer.locator(".crm-register-card").click();
  await drawer.getByLabel("更新阶段").selectOption("won");
  await expect(
    drawer.getByRole("heading", { name: "成交转客户" }),
  ).toBeVisible();
  await drawer.getByLabel("本次沟通").fill("合同已签署");
  await drawer.getByRole("button", { name: "确认成交并保存档案" }).click();
  expect(attempts).toBe(0);
  await drawer.getByLabel("正式联系方式", { exact: true }).fill("13800138000");
  await drawer.getByLabel("正式客户", { exact: true }).selectOption("customer");
  await expect(drawer.getByLabel("正式客户名称", { exact: true })).toHaveCount(
    0,
  );
  await drawer.getByRole("button", { name: "确认成交并保存档案" }).click();
  await expect(drawer.getByRole("alert")).toContainText("记录已更新");
  await expect(drawer.getByLabel("正式联系方式", { exact: true })).toHaveValue(
    "13800138000",
  );
  await drawer.getByRole("button", { name: "确认成交并保存档案" }).click();
  await expect(drawer).not.toBeVisible();
  await expect(
    page.getByRole("status").filter({ hasText: "跟进已保存" }),
  ).toBeVisible();
  expect(attempts).toBe(2);
});
