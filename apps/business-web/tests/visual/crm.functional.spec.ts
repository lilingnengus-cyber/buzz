import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("售前 CRM 新建、跟进、筛选和刷新", async ({ page }) => {
  let records: any[] = [];
  let notes: any[] = [];
  let writes = 0;
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      url = new URL(req.url()),
      path = url.pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: {
          authenticated: true,
          displayName: "CRM 测试",
          csrfToken: "test-csrf",
        },
      });
    if (path === "/api/v1/crm/options")
      return route.fulfill({
        json: {
          items: [
            { id: "legal", resourceType: "legal_entity", name: "默认法人主体" },
            {
              id: "unit",
              resourceType: "business_unit",
              name: "默认业务单元",
              legalEntityId: "legal",
            },
            {
              id: "customer",
              resourceType: "customer",
              name: "杭州示例企业",
              legalEntityId: "legal",
              businessUnitId: "unit",
            },
          ],
        },
      });
    if (req.method() !== "GET") {
      expect(req.headers()["x-csrf-token"]).toBe("test-csrf");
      expect(req.headers()["idempotency-key"]).toBeTruthy();
      writes++;
      const input = req.postDataJSON();
      if (path.endsWith("/followups")) {
        expect(input.expectedVersion).toBe(records[0].version);
        notes.unshift({
          ...input,
          id: "note-1",
          authorName: "CRM 测试",
          createdAt: "2026-09-19T02:00:00Z",
        });
        records[0] = {
          ...records[0],
          stage: input.stage,
          nextAction: input.nextAction,
          nextFollowUp: input.nextFollowUp,
          version: records[0].version + 1,
        };
      } else if (req.method() === "POST")
        records.push({
          ...input,
          id: "opp-1",
          version: 1,
          createdAt: "2026-09-19T01:00:00Z",
          updatedAt: "2026-09-19T01:00:00Z",
        });
      else
        records[0] = {
          ...records[0],
          ...input,
          version: records[0].version + 1,
        };
      return route.fulfill({
        json: { id: "opp-1", version: records[0].version },
      });
    }
    if (path === "/api/v1/crm/followups")
      return route.fulfill({
        json: {
          items: notes.map((n) => ({
            ...n,
            opportunityId: "opp-1",
            opportunityTitle: records[0].title,
            companyName: records[0].companyName,
            contactName: records[0].contactName,
          })),
          hasMore: false,
        },
      });
    if (path === "/api/v1/crm/contacts")
      return route.fulfill({
        json: {
          items: [
            {
              companyName: records[0].companyName,
              contactName: records[0].contactName,
              contactDetails: "",
              opportunities: [{ id: "opp-1", title: records[0].title }],
            },
          ],
          hasMore: false,
        },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: {
          items: records.filter(
            (r) =>
              (!url.searchParams.get("stage") ||
                r.stage === url.searchParams.get("stage")) &&
              (!url.searchParams.get("query") ||
                r.companyName.includes(url.searchParams.get("query"))),
          ),
          canManage: true,
          hasMore: false,
        },
      });
    if (path === "/api/v1/crm/opportunities/opp-1")
      return route.fulfill({
        json: { item: records[0], followups: notes, hasOlderFollowups: false },
      });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crm");
  await expect(
    page.getByRole("heading", { name: "商机", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "新建商机", exact: true }).click();
  const form = page.getByRole("complementary", { name: "新建商机" });
  await form.getByLabel("商机名称").fill("企业年度采购");
  await form.getByLabel("关联已有客户").selectOption("customer");
  await expect(form.getByLabel("客户公司")).toHaveValue("杭州示例企业");
  await form.getByLabel("联系人", { exact: true }).fill("陈经理");
  await form.getByLabel("预计金额").fill("1250.50");
  await form.getByLabel("下一步", { exact: true }).fill("发送初步方案");
  await form.getByLabel("下次跟进日期").fill("2026-09-20");
  await form.getByRole("button", { name: "保存商机", exact: true }).click();
  const detail = page.getByRole("complementary", { name: "商机详情" });
  await expect(
    detail.getByRole("heading", { name: "企业年度采购" }),
  ).toBeVisible();
  expect(records[0].expectedAmountMinor).toBe(125050);
  expect(writes).toBe(1);
  await detail.getByLabel("本次沟通").fill("客户确认需求，准备报价。");
  await detail.getByLabel("更新阶段").selectOption("quoting");
  await detail.getByLabel("下一步", { exact: true }).fill("提交报价单");
  await detail.getByRole("button", { name: "保存跟进", exact: true }).click();
  await expect(
    detail.getByText("客户确认需求，准备报价。", { exact: true }),
  ).toBeVisible();
  expect(writes).toBe(2);
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(
    detail.getByText("客户确认需求，准备报价。", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("正在加载商机…", { exact: true })).toHaveCount(0);
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-desktop.png" });
  await page.setViewportSize({ width: 800, height: 900 });
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-compact.png" });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.goto("/#crmFollowups");
  await expect(
    page.getByRole("heading", { name: "跟进记录", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("客户确认需求，准备报价。", { exact: true }),
  ).toBeVisible();
  await page.getByRole("link", { name: "企业年度采购", exact: true }).click();
  await expect(
    detail.getByRole("heading", { name: "企业年度采购" }),
  ).toBeVisible();
  await page.goto("/#crmContacts");
  await expect(
    page.getByRole("heading", { name: "客户联系人", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "陈经理" })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "陈经理" })).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-contacts.png" });
  await page.getByRole("link", { name: "企业年度采购", exact: true }).click();
  await expect(
    detail.getByRole("heading", { name: "企业年度采购" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "收起详情" }).click();
  await page
    .getByRole("combobox", { name: "阶段", exact: true })
    .selectOption("lost");
  await expect(
    page.getByRole("heading", { name: "没有符合条件的商机" }),
  ).toBeVisible();
});

test("CRM 读取失败显示重试而不是空列表", async ({ page }) => {
  await page.route("**/api/**", (route) =>
    route.fulfill({ status: 503, json: { message: "暂时无法读取商机" } }),
  );
  await page.goto("/#crm");
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "重新加载", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "新建商机", exact: true }),
  ).toHaveCount(0);
});
