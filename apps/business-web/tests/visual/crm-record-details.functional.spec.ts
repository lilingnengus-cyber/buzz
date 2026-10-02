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
  await contactRow.focus();
  await page.keyboard.press("Enter");
  const contactDetail = page.getByRole("dialog", {
    name: "联系人详情",
    exact: true,
  });
  await expect(contactDetail).toBeVisible();
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
  await page
    .getByRole("navigation", { name: "档案类型" })
    .getByRole("button", { name: "客户", exact: true })
    .click();
  await page
    .getByRole("button", { name: "查看客户：潜在客户", exact: true })
    .getByRole("heading")
    .click();
  const customerDetail = page.getByRole("dialog", {
    name: "客户详情",
    exact: true,
  });
  await expect(
    customerDetail.getByRole("heading", { name: "潜在客户", exact: true }),
  ).toBeVisible();
  await customerDetail.getByRole("button", { name: "编辑客户" }).click();
  await expect(
    page
      .getByRole("dialog", { name: "编辑客户", exact: true })
      .getByLabel("客户名称"),
  ).toHaveValue("潜在客户");
  await page.keyboard.press("Escape");
  await page
    .getByRole("button", { name: "查看客户：已有核心客户", exact: true })
    .getByRole("heading")
    .click();
  await expect(customerDetail).toBeVisible();
  await expect(
    customerDetail.getByRole("button", { name: "编辑客户" }),
  ).toHaveCount(0);
  await expect(
    customerDetail.getByRole("link", { name: "前往核心数据维护" }),
  ).toBeVisible();
  await page.setViewportSize({ width: 520, height: 900 });
  const narrow = await customerDetail.boundingBox();
  expect(Math.round(narrow!.x)).toBe(0);
  expect(Math.round(narrow!.width)).toBe(520);
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/crm-customer-record-detail-520.png",
  });
  await page.keyboard.press("Escape");
  await expect(customerDetail).not.toBeVisible();
  expect(writes).toHaveLength(1);
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
  await detail.getByRole("link", { name: "打开商机继续跟进" }).click();
  await expect(page).toHaveURL(/#crm\?opportunity=opp$/);
  expect(writes).toHaveLength(0);
});

test("客户详情关联记录按客户 ID 隔离，联系人可直接编辑", async ({ page }) => {
  await seedRecords(page, true);
  await page.goto("/#crmContacts");
  await page.getByRole("button", { name: "客户", exact: true }).click();
  await page
    .getByRole("button", { name: "查看客户：潜在客户", exact: true })
    .click();
  const detail = page.getByRole("dialog", { name: "客户详情", exact: true });
  await expect(
    detail.getByRole("link", { name: "年度采购 · 沟通中" }),
  ).toHaveAttribute("href", "/#crm?opportunity=opp");
  await expect(detail.getByText("同名客户商机")).toHaveCount(0);
  await detail.getByRole("button", { name: "张经理 · 13800000000" }).click();
  const contact = page.getByRole("dialog", { name: "联系人详情", exact: true });
  await expect(contact).toBeVisible();
  await contact.getByRole("button", { name: "编辑联系人" }).click();
  await expect(
    page
      .getByRole("dialog", { name: "编辑联系人", exact: true })
      .getByLabel("联系方式", { exact: true }),
  ).toHaveValue("13800000000");
});
