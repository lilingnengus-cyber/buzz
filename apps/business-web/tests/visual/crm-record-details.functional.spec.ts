import { expect, test, type Page } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

async function seedRecords(page: Page, canManage: boolean) {
  const accounts = [
    { id: "prospect", name: "潜在客户", customerId: null, version: 1 },
    {
      id: "core-account",
      name: "已有核心客户",
      customerId: "core-customer",
      version: 1,
    },
  ];
  const contacts = [
    {
      id: "contact",
      accountId: "prospect",
      customerId: "core-customer",
      companyName: "潜在客户",
      contactName: "张经理",
      contactDetails: "13800000000",
      version: 1,
      opportunities: [{ id: "opp", title: "年度采购" }],
    },
  ];
  const notes = [
    {
      id: "note",
      opportunityId: "opp",
      opportunityTitle: "年度采购",
      companyName: "潜在客户",
      contactName: "张经理",
      note: "已发送方案\n等待客户反馈",
      authorName: "销售同事",
      createdAt: "2026-10-02T01:00:00Z",
      stage: "contacting",
      nextAction: "电话确认方案",
      nextFollowUp: "2026-10-05",
    },
  ];
  const writes: string[] = [];
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      path = new URL(req.url()).pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf", displayName: "CRM" },
      });
    if (req.method() !== "GET") {
      writes.push(path);
      expect(req.headers()["x-csrf-token"]).toBe("csrf");
      expect(req.headers()["idempotency-key"]).toBeTruthy();
      const body = req.postDataJSON();
      expect(body.expectedVersion).toBe(1);
      if (path.endsWith("/contacts/contact")) {
        contacts[0].contactDetails = body.details;
        contacts[0].version++;
      }
      return route.fulfill({ json: { id: "contact", version: 2 } });
    }
    if (path === "/api/v1/crm/accounts")
      return route.fulfill({
        json: { items: accounts, hasMore: false, canManage },
      });
    if (path === "/api/v1/crm/contacts")
      return route.fulfill({
        json: { items: contacts, hasMore: false, canManage },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: {
          items: [
            {
              id: "opp",
              accountId: "prospect",
              title: "年度采购",
              stage: "contacting",
            },
            {
              id: "other",
              accountId: "other",
              title: "同名客户商机",
              stage: "won",
            },
          ],
          hasMore: false,
        },
      });
    if (path === "/api/v1/crm/followups")
      return route.fulfill({ json: { items: notes, hasMore: false } });
    return route.fulfill({ json: { items: [] } });
  });
  return writes;
}

test("客户与联系人点击记录打开右侧详情，编辑保留未保存保护", async ({
  page,
}) => {
  const writes = await seedRecords(page, true);
  await page.goto("/#crmContacts");
  const contactRow = page.getByRole("button", {
    name: "查看联系人：张经理",
    exact: true,
  });
  await expect(
    contactRow.getByRole("link", { name: "潜在客户", exact: true }),
  ).toHaveAttribute("href", "/customers/core-customer");
  await contactRow.focus();
  await page.keyboard.press("Enter");
  const contactDetail = page.getByRole("dialog", {
    name: "联系人详情",
    exact: true,
  });
  await expect(contactDetail).toBeVisible();
  await expect(
    contactDetail.getByRole("link", { name: "潜在客户", exact: true }),
  ).toHaveAttribute("href", "/customers/core-customer");
  await expect(
    contactDetail.getByText("13800000000", { exact: true }),
  ).toBeVisible();
  await expect(
    contactDetail.getByRole("link", { name: "年度采购" }),
  ).toHaveAttribute("href", "/#crm?opportunity=opp");
  const box = await contactDetail.boundingBox();
  expect(Math.round(box!.x + box!.width)).toBe(page.viewportSize()!.width);
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-contact-record-detail.png" });
  await page.mouse.click(10, 450);
  await expect(contactDetail).not.toBeVisible();
  await contactRow.getByRole("heading", { name: "张经理" }).click();
  await contactDetail.getByRole("button", { name: "编辑联系人" }).click();
  const edit = page.getByRole("dialog", { name: "编辑联系人", exact: true });
  await edit.getByLabel("联系方式", { exact: true }).fill("13900000000");
  await page.keyboard.press("Escape");
  const confirm = page.getByRole("dialog", { name: "放弃未保存修改" });
  await expect(confirm).toBeVisible();
  await confirm.getByRole("button", { name: "继续编辑" }).click();
  await expect(edit.getByLabel("联系方式", { exact: true })).toHaveValue(
    "13900000000",
  );
  await edit.getByRole("button", { name: "保存档案" }).click();
  await expect(edit).not.toBeVisible();
  expect(writes).toEqual(["/api/v1/crm/contacts/contact"]);
  await expect(
    page.getByRole("heading", { name: "联系人", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("navigation", { name: "档案类型" })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("button", { name: "新建客户", exact: true }),
  ).toHaveCount(0);
  await contactRow.click();
  await page.setViewportSize({ width: 520, height: 900 });
  await expect(contactDetail).toBeVisible();
  expect(Math.round((await contactDetail.boundingBox())!.width)).toBe(520);
  await contactDetail
    .getByRole("link", { name: "潜在客户", exact: true })
    .click();
  await expect(page).toHaveURL(/\/customers\/core-customer$/);
});

test("只读联系人也可查看右侧详情", async ({ page }) => {
  const writes = await seedRecords(page, false);
  await page.goto("/#crmContacts");
  await page
    .getByRole("button", { name: "查看联系人：张经理", exact: true })
    .getByRole("heading")
    .click();
  const detail = page.getByRole("dialog", { name: "联系人详情", exact: true });
  await expect(detail).toBeVisible();
  await expect(detail.getByRole("button", { name: "编辑联系人" })).toHaveCount(
    0,
  );
  await expect(detail.getByRole("link", { name: "年度采购" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).not.toBeVisible();
  expect(writes).toHaveLength(0);
});

test("跟进记录采用右侧详情，保留商机链接且不修改历史", async ({ page }) => {
  const writes = await seedRecords(page, false);
  await page.goto("/#crmFollowups");
  const row = page.getByRole("button", {
    name: "查看跟进：年度采购",
    exact: true,
  });
  await row.getByText("已发送方案\n等待客户反馈").click();
  const detail = page.getByRole("dialog", {
    name: "跟进记录详情",
    exact: true,
  });
  await expect(detail).toBeVisible();
  await expect(detail.getByRole("heading", { name: "年度采购" })).toBeVisible();
  await expect(detail.getByText("电话确认方案", { exact: true })).toBeVisible();
  await expect(detail.getByText("2026-10-05", { exact: true })).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/crm-followup-record-detail.png",
  });
  await page.mouse.click(10, 450);
  await expect(detail).not.toBeVisible();
  await row.focus();
  await page.keyboard.press("Space");
  await expect(detail).toBeVisible();
  await detail.getByRole("link", { name: "查看关联商机" }).click();
  await expect(page).toHaveURL(/#crm\?opportunity=opp$/);
  expect(writes).toHaveLength(0);
});

test("跟进与联系人列表分列展示，窄屏没有横向溢出", async ({ page }) => {
  const writes = await seedRecords(page, true);
  await page.goto("/#crmFollowups");
  await expect(
    page.getByRole("button", { name: "查看跟进：年度采购", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".crm-register-columns")).toContainText("下一步");
  await expect(page.locator(".crm-register-columns")).toContainText("跟进日期");
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-followup-list.png" });
  await expect(
    page.locator(".crm-followup-grid.crm-register-columns > span"),
  ).toHaveText([
    "商机",
    "客户",
    "联系人",
    "阶段",
    "沟通内容",
    "下一步",
    "跟进日期",
    "记录人",
    "记录时间",
    "流失原因",
  ]);
  await page.setViewportSize({ width: 520, height: 900 });
  await expect(
    page.locator(".crm-followup-grid.crm-register-columns"),
  ).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(520);
  expect(
    await page
      .locator(".crm-followup-scroll")
      .evaluate((el) => el.scrollWidth > el.clientWidth),
  ).toBe(true);
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-followup-columns-520.png" });
  await page.setViewportSize({ width: 1366, height: 768 });
  await page.goto("/#crmContacts");
  await expect(
    page.getByRole("button", { name: "查看联系人：张经理", exact: true }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-contact-list.png" });
  await page.setViewportSize({ width: 520, height: 900 });
  await expect(page.locator(".crm-register-columns")).not.toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(520);
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-contact-list-520.png" });
  expect(writes).toEqual([]);
});

test("客户筛选与搜索同时发送，清除后恢复全部客户", async ({ page }) => {
  await seedRecords(page, true);
  for (const hash of ["crmFollowups", "crmContacts"]) {
    await page.goto(`/#${hash}`);
    const select = page.getByRole("combobox", {
      name: "按客户筛选",
      exact: true,
    });
    await select.click();
    await expect(page.getByRole("option", { name: /潜在客户/ }).last()).toBeVisible();
    const path = hash === "crmContacts" ? "contacts" : "followups";
    const filtered = page.waitForRequest(
      (r) =>
        r.url().includes(`/crm/${path}?`) &&
        new URL(r.url()).searchParams.get("accountId") === "prospect",
    );
    await page.getByRole("option", { name: /潜在客户/ }).last().click();
    expect(new URL((await filtered).url()).searchParams.get("offset")).toBe(
      "0",
    );
    const searched = page.waitForRequest(
      (r) =>
        r.url().includes(`/crm/${path}?`) &&
        new URL(r.url()).searchParams.get("query") === "张经理",
    );
    await page
      .getByRole("searchbox", {
        name: hash === "crmContacts" ? "搜索联系人" : "搜索跟进记录",
        exact: true,
      })
      .fill("张经理");
    expect(new URL((await searched).url()).searchParams.get("accountId")).toBe(
      "prospect",
    );
    const cleared = page.waitForRequest(
      (r) =>
        r.url().includes(`/crm/${path}?`) &&
        !new URL(r.url()).searchParams.has("accountId"),
    );
    await select.click();
    await page.getByRole("option", { name: "全部客户", exact: true }).click();
    expect(new URL((await cleared).url()).searchParams.get("query")).toBe(
      "张经理",
    );
  }
});

test("历史潜在客户联系人不生成核心客户链接", async ({ page }) => {
  await seedRecords(page, false);
  await page.route("**/api/v1/crm/contacts?**", (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: "legacy",
            accountId: "prospect",
            customerId: null,
            companyName: "历史潜客",
            contactName: "李经理",
            contactDetails: "",
            opportunities: [],
          },
        ],
        canManage: false,
        hasMore: false,
      },
    }),
  );
  await page.goto("/#crmContacts");
  const row = page.getByRole("button", {
    name: "查看联系人：李经理",
    exact: true,
  });
  await expect(row).toBeVisible();
  await expect(row.getByRole("link")).toHaveCount(0);
  await row.click();
  await expect(
    page.getByRole("dialog").getByText("历史潜客", { exact: true }).first(),
  ).toBeVisible();
  await expect(page.getByRole("dialog").getByRole("link")).toHaveCount(0);
});

test("跟进权限失败保留历史，独立重试和刷新可恢复录入", async ({ page }) => {
  await seedRecords(page, true);
  let fails = true;
  await page.route("**/api/v1/crm/opportunities?offset=0", (route) => fails
    ? route.fulfill({ status: 503, json: { message: "权限服务暂不可用" } })
    : route.fulfill({ json: { items: [], hasMore: false, canManage: true } }));
  await page.goto("/#crmFollowups");
  await expect(page.getByRole("button", { name: "查看跟进：年度采购" })).toBeVisible();
  await expect(page.getByRole("alert")).toContainText("跟进录入权限读取失败");
  await expect(page.getByRole("button", { name: "新建跟进", exact: true })).toHaveCount(0);
  fails = false;
  await page.getByRole("button", { name: "重试录入权限" }).click();
  await expect(page.getByRole("button", { name: "新建跟进", exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  fails = true;
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("跟进录入权限读取失败");
  await expect(page.getByRole("button", { name: "新建跟进", exact: true })).toHaveCount(0);
  fails = false;
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(page.getByRole("button", { name: "新建跟进", exact: true })).toBeVisible();
});
