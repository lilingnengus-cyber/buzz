import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("客户联系人独立建档、复用及未保存保护", async ({ page }) => {
  const accounts: any[] = [];
  const contacts: any[] = [];
  const opportunities: any[] = [];
  let failContact = false;
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      url = new URL(req.url()),
      path = url.pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: {
          authenticated: true,
          csrfToken: "csrf",
          displayName: "CRM 测试",
        },
      });
    if (path === "/api/v1/crm/options")
      return route.fulfill({
        json: {
          items: [
            {
              id: "legal",
              name: "法人",
              code: "LE",
              resourceType: "legal_entity",
              status: "active",
            },
            {
              id: "unit",
              name: "经营单元",
              code: "OU",
              resourceType: "business_unit",
              status: "active",
              ancestorPath: ["经营单元"],
              depth: 0,
              descendantCount: 0,
            },
          ],
        },
      });
    if (path.startsWith("/api/v1/crm/accounts") && req.method() !== "GET") {
      const input = req.postDataJSON();
      accounts.push({ ...input, id: "account-1", version: 1 });
      return route.fulfill({ json: { id: "account-1", version: 1 } });
    }
    if (path.startsWith("/api/v1/crm/contacts") && req.method() !== "GET") {
      if (failContact)
        return route.fulfill({
          status: 409,
          json: { message: "记录已更新，请刷新后重试" },
        });
      const input = req.postDataJSON();
      contacts.push({
        id: "contact-1",
        accountId: input.accountId,
        companyName: accounts[0].name,
        contactName: input.name,
        contactDetails: input.details,
        version: 1,
        opportunities: [],
      });
      return route.fulfill({ json: { id: "contact-1", version: 1 } });
    }
    if (path === "/api/v1/crm/accounts")
      return route.fulfill({
        json: { items: accounts, hasMore: false, canManage: true },
      });
    if (path === "/api/v1/crm/contacts")
      return route.fulfill({
        json: { items: contacts, hasMore: false, canManage: true },
      });
    if (path === "/api/v1/crm/opportunities" && req.method() === "POST") {
      const input = req.postDataJSON();
      opportunities.push({ ...input, id: "opp-1", version: 1 });
      return route.fulfill({ json: { id: "opp-1", version: 1 } });
    }
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: { items: opportunities, hasMore: false, canManage: true },
      });
    if (path === "/api/v1/crm/opportunities/opp-1")
      return route.fulfill({
        json: {
          item: opportunities[0],
          followups: [],
          hasOlderFollowups: false,
        },
      });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crmContacts");
  await page
    .getByRole("navigation", { name: "档案类型" })
    .getByRole("button", { name: "客户", exact: true })
    .click();
  await page.getByRole("button", { name: "新建客户", exact: true }).click();
  let drawer = page.getByRole("dialog");
  await drawer.getByLabel("客户名称").fill("独立潜在客户");
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "放弃未保存修改" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "继续编辑" }).click();
  await expect(drawer).toBeVisible();
  await expect(drawer.getByLabel("客户名称")).toHaveValue("独立潜在客户");
  await drawer.getByRole("button", { name: "保存档案" }).click();
  await expect(drawer).not.toBeVisible();
  await expect(
    page.getByRole("heading", { name: "独立潜在客户" }),
  ).toBeVisible();
  expect(accounts[0]).not.toHaveProperty("legalEntityId");
  await page
    .getByRole("navigation", { name: "档案类型" })
    .getByRole("button", { name: "联系人", exact: true })
    .click();
  await page.getByRole("button", { name: "新建联系人" }).click();
  await drawer
    .getByLabel("客户档案", { exact: true })
    .selectOption("account-1");
  await drawer.getByLabel("联系人姓名").fill("张经理");
  await drawer
    .getByLabel("联系方式", { exact: true })
    .fill("zhang@example.test");
  await page.setViewportSize({ width: 520, height: 900 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-contact-drawer-520.png" });
  await page.setViewportSize({ width: 1366, height: 900 });
  failContact = true;
  await drawer.getByRole("button", { name: "保存档案" }).click();
  await expect(drawer.getByRole("alert")).toContainText("记录已更新");
  await page.mouse.click(10, 450);
  await page.getByRole("button", { name: "继续编辑" }).click();
  await expect(drawer).toBeVisible();
  failContact = false;
  await drawer.getByRole("button", { name: "保存档案" }).click();
  await expect(drawer).not.toBeVisible();
  await expect(page.getByRole("heading", { name: "张经理" })).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-independent-contacts.png" });
  await page.goto("/#crm");
  await page.getByRole("button", { name: "新建商机", exact: true }).click();
  await drawer
    .getByLabel("客户档案", { exact: true })
    .selectOption("account-1");
  await drawer
    .getByLabel("选择联系人", { exact: true })
    .selectOption("contact-1");
  await drawer.getByLabel("商机名称").fill("年度项目");
  await expect(drawer.getByLabel("客户公司")).toHaveValue("独立潜在客户");
  await expect(drawer.getByLabel("联系人", { exact: true })).toHaveValue(
    "张经理",
  );
  await drawer.getByRole("button", { name: "保存商机", exact: true }).click();
  await expect(drawer.getByRole("heading", { name: "年度项目" })).toBeVisible();
  expect(opportunities[0].accountId).toBe("account-1");
  expect(opportunities[0].contactId).toBe("contact-1");
  await expect(drawer.getByLabel("本次沟通")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.goto("/#crmFollowups");
  await page.getByRole("button", { name: "新建跟进", exact: true }).click();
  const followup = page.getByRole("dialog", { name: "新建跟进", exact: true });
  await followup.getByRole("button", { name: /年度项目/ }).click();
  await followup.getByLabel("本次沟通").fill("尚未提交的沟通");
  await followup.getByRole("button", { name: "更换商机" }).click();
  await page.getByRole("button", { name: "继续编辑" }).click();
  await expect(followup.getByLabel("本次沟通")).toHaveValue("尚未提交的沟通");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await expect(followup).not.toBeVisible();
  await page.goto("/#crm");
  for (const value of ["overdue", "today", "upcoming", "unscheduled", ""]) {
    const fetched = page.waitForRequest(
      (req) =>
        req.url().includes("/api/v1/crm/opportunities?") &&
        (new URL(req.url()).searchParams.get("followup") ?? "") === value,
    );
    await page.getByLabel("跟进安排").selectOption(value);
    const req = await fetched;
    if (value)
      expect(new URL(req.url()).searchParams.get("today")).toMatch(
        /^\d{4}-\d{2}-\d{2}$/,
      );
  }
});

test("只读联系人档案不展示写入入口", async ({ page }) => {
  await page.route("**/api/**", (route) =>
    route.fulfill({
      json: route.request().url().includes("/crm/contacts")
        ? {
            items: [
              {
                id: "c1",
                accountId: "a1",
                contactName: "只读联系人",
                companyName: "客户",
                contactDetails: "",
                opportunities: [],
                version: 1,
              },
            ],
            hasMore: false,
            canManage: false,
          }
        : { authenticated: true, items: [] },
    }),
  );
  await page.goto("/#crmContacts");
  await expect(page.getByRole("heading", { name: "只读联系人" })).toBeVisible();
  await expect(page.getByRole("button", { name: "新建联系人" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "编辑联系人" })).toHaveCount(0);
});
