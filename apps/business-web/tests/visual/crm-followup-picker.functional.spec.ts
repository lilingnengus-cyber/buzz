import { expect, test } from "@playwright/test";

test("跟进商机下拉框内搜索、分页与键盘选择", async ({ page }) => {
  const requests: URL[] = [];
  const item = { id: "deal", title: "北京采购", companyName: "示例客户", contactName: "张经理", stage: "contacting", version: 1, nextAction: "", nextFollowUp: null };
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    let json: unknown = { items: [], canManage: true };
    if (url.pathname === "/api/session") json = { authenticated: true, csrfToken: "csrf", displayName: "测试" };
    if (url.pathname === "/api/v1/crm/opportunities") {
      requests.push(url);
      json = { canManage: true, items: [item], hasMore: url.searchParams.get("offset") !== "50" };
    }
    if (url.pathname === "/api/v1/crm/opportunities/deal") json = { item, followups: [] };
    await route.fulfill({ json });
  });
  await page.goto("/#crmFollowups");
  await page.getByRole("button", { name: "新建跟进", exact: true }).click();
  const drawer = page.getByRole("dialog", { name: "新建跟进", exact: true });
  const trigger = drawer.getByRole("combobox", { name: "关联商机", exact: true });
  await expect(drawer.getByRole("combobox", { name: "搜索关联商机", exact: true })).toHaveCount(0);
  await trigger.click();
  await drawer.getByRole("button", { name: "下一页", exact: true }).click();
  await expect.poll(() => requests.at(-1)?.searchParams.get("offset")).toBe("50");
  const search = drawer.getByRole("combobox", { name: "搜索关联商机", exact: true });
  await search.fill("北京");
  await expect.poll(() => requests.at(-1)?.searchParams.get("query")).toBe("北京");
  expect(requests.at(-1)?.searchParams.get("offset")).toBe("0");
  await search.press("Escape");
  await expect(drawer).toBeVisible();
  await expect(trigger).toBeFocused();
  await trigger.press("ArrowDown");
  await expect(drawer.getByRole("option", { name: /北京采购/ })).toBeVisible();
  await search.press("Enter");
  await expect(drawer.getByRole("heading", { name: "北京采购", exact: true })).toBeVisible();
  await expect(drawer.getByLabel("本次沟通")).toBeVisible();
});
