import { expect, test } from "@playwright/test";

test("录入选项失败不隐藏商机和详情，独立重试恢复写入入口", async ({ page }) => {
  let optionFails = true;
  let listReads = 0;
  const record = { id: "op", legalEntityId: "le", businessUnitId: "bu", customerId: null, title: "年度服务项目", companyName: "客户甲", contactName: "张经理", contactDetails: "", stage: "contacting", expectedAmountMinor: null, currency: "CNY", nextAction: "", nextFollowUp: null, version: 1, createdAt: "2026-10-03T00:00:00Z", updatedAt: "2026-10-03T00:00:00Z" };
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "test" } });
    if (path === "/api/v1/crm/options") return optionFails ? route.fulfill({ status: 503, json: { message: "选项暂不可用" } }) : route.fulfill({ json: { items: [] } });
    if (path === "/api/v1/crm/opportunities") { listReads += 1; return route.fulfill({ json: { items: [record], hasMore: false, canManage: true } }); }
    if (path === "/api/v1/crm/opportunities/op") return route.fulfill({ json: { item: record, followups: [], hasOlderFollowups: false } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crm");
  await expect(page.locator('.crm-row')).toContainText("年度服务项目");
  await expect(page.getByRole("alert")).toContainText("录入选项读取失败");
  await expect(page.getByRole("button", { name: "新建商机", exact: true })).toHaveCount(0);
  await page.locator('.crm-row').click();
  await expect(page.getByRole("dialog")).toContainText("年度服务项目");
  await expect(page.getByRole("dialog").getByRole("button", { name: "编辑商机" })).toHaveCount(0);
  await page.getByRole("button", { name: "关闭详情弹窗" }).click();
  const reads = listReads;
  optionFails = false;
  await page.getByRole("button", { name: "重试录入选项" }).click();
  await expect(page.getByRole("button", { name: "新建商机", exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(listReads).toBe(reads);
});

test("客户候选搜索加载时不保留旧结果，也不误报没有匹配", async ({ page }) => {
  let release: (() => void) | undefined;
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url()), path = url.pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "test" } });
    if (path === "/api/v1/crm/accounts") {
      if (url.searchParams.get("query") === "Beta") {
        await new Promise<void>((resolve) => { release = resolve; });
        return route.fulfill({ json: { items: [{ id: "b", name: "Beta", customerId: null }], hasMore: false } });
      }
      return route.fulfill({ json: { items: [{ id: "a", name: "Alpha", customerId: null }], hasMore: false } });
    }
    return route.fulfill({ json: { items: [], hasMore: false, canManage: true } });
  });
  await page.goto("/#crmContacts");
  await page.getByRole("button", { name: "新建联系人", exact: true }).click();
  const drawer = page.getByRole("dialog");
  await drawer.getByRole("combobox", { name: "客户档案", exact: true }).click();
  await expect(drawer.getByRole("option", { name: "Alpha（潜在客户）", exact: true })).toBeVisible();
  await drawer.getByRole("combobox", { name: "搜索客户档案", exact: true }).fill("Beta");
  await expect(drawer.getByRole("option", { name: "Alpha（潜在客户）", exact: true })).toHaveCount(0);
  await expect(drawer.getByRole("status")).toContainText("正在读取候选");
  await expect(drawer).not.toContainText("暂无匹配结果");
  await expect.poll(() => typeof release).toBe("function");
  release!();
  await expect(drawer.getByRole("option", { name: "Beta（潜在客户）", exact: true })).toBeVisible();
  await expect(drawer.getByRole("status")).toHaveCount(0);
});
