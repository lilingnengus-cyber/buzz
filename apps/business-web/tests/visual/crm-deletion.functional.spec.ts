import { test, expect } from "@playwright/test";

for (const canManage of [true, false]) {
  test(`商机删除确认、失败保留和权限 canManage=${canManage}`, async ({ page }) => {
    const item = { id: "opp", title: "待删除商机", companyName: "公司甲", stage: "new", currency: "CNY", version: 3, contactName: "张经理", contactDetails: "", nextAction: "发送方案", nextFollowUp: null };
    let deleted = false;
    let attempts = 0;
    const keys: string[] = [];
    await page.route("**/api/**", async (route) => {
      const req = route.request();
      const path = new URL(req.url()).pathname;
      if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf" } });
      if (req.method() === "DELETE") {
        attempts++;
        expect(req.postDataJSON()).toEqual({ expectedVersion: 3 });
        keys.push(req.headers()["idempotency-key"]);
        if (attempts === 1) return route.fulfill({ status: 409, json: { message: "记录已更新，请刷新后重试", code: "VERSION_CONFLICT" } });
        deleted = true;
        return route.fulfill({ json: { id: "opp", deleted: true, version: 4 } });
      }
      if (path === "/api/v1/crm/opportunities/opp") return route.fulfill({ json: { item, followups: [], hasOlderFollowups: false } });
      if (path === "/api/v1/crm/opportunities") return route.fulfill({ json: { items: deleted ? [] : [item], canManage, hasMore: false } });
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto("/#crm?opportunity=opp");
    const drawer = page.getByRole("dialog", { name: "商机详情", exact: true });
    await expect(drawer).toBeVisible();
    const remove = drawer.getByRole("button", { name: "删除商机", exact: true });
    if (!canManage) { await expect(remove).toBeHidden(); return; }
    await remove.click();
    const confirmation = page.getByRole("dialog", { name: "删除商机确认" });
    await expect(confirmation).toBeVisible();
    await confirmation.getByRole("button", { name: "取消", exact: true }).click();
    expect(attempts).toBe(0);
    await expect(drawer).toBeVisible();
    await remove.click();
    await confirmation.getByRole("button", { name: "确认删除" }).click();
    await expect(confirmation.getByRole("alert")).toContainText("记录已更新");
    await expect(drawer).toBeVisible();
    await confirmation.getByRole("button", { name: "确认删除" }).click();
    await expect(drawer).toBeHidden();
    await expect(page.getByText("商机已删除", { exact: true })).toBeVisible();
    expect(keys[0]).toBeTruthy();
    expect(keys[0]).toBe(keys[1]);
    await expect(page.getByText("待删除商机", { exact: true })).toBeHidden();
  });
}
