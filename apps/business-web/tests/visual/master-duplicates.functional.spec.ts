import { expect, test } from "@playwright/test";

for (const [type, label, path] of [["customer", "客户", "customers"], ["supplier", "供应商", "suppliers"]]) {
  test(`${label}跨页重复提醒、编辑排除自身、失败重试`, async ({ page }) => {
    const base = { resourceType: type, status: "active", version: 1, updatedAt: "2026-10-05T00:00:00Z", creditCurrency: "CNY", creditLimitMinor: 0, paymentTermsDays: 30 };
    const self = { ...base, id: "self", code: "SELF", name: "独立公司" };
    const duplicate = { ...base, id: "other", code: "OTHER", name: "ＡＢＣ 公司", status: "disabled" };
    let fail = true;
    let writes = 0;
    await page.route("**/api/**", async (route) => {
      const url = new URL(route.request().url());
      if (route.request().method() !== "GET") writes++;
      if (url.pathname === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf" } });
      if (url.pathname.endsWith("core-master-data")) {
        const candidates = url.searchParams.get("limit") === "500";
        if (candidates && fail) return route.fulfill({ status: 503, json: { error: "unavailable" } });
        const second = url.searchParams.get("offset") === "1";
        return route.fulfill({ json: { items: candidates && second ? [duplicate] : [self], dataAsOf: "2026-10-05T00:00:00Z", canManage: true, hasMore: candidates && !second, total: 2 } });
      }
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto(`/${path}/self`);
    const dialog = page.getByRole("dialog", { name: `${label}详情`, exact: true });
    await expect(dialog.getByText("暂未完成重复检查，请自行核对。")).toBeVisible();
    fail = false;
    await dialog.getByRole("button", { name: "重试重复检查" }).click();
    await expect(dialog.getByText("暂未完成重复检查，请自行核对。")).toHaveCount(0);
    await expect(dialog.getByText(/可能重复的/)).toHaveCount(0);
    const name = dialog.getByRole("textbox", { name: "名称 *", exact: true });
    await name.fill("abc公司");
    await expect(dialog.getByText(`发现 1 条可能重复的${label}`)).toBeVisible();
    await expect(dialog.getByText(/OTHER.*同名.*已停用/)).toBeVisible();
    await expect(dialog.getByRole("button", { name: "保存修订" })).toBeEnabled();
    await name.fill("abc公司杭州分部");
    await expect(dialog.getByText(/OTHER.*名称相近/)).toBeVisible();
    await name.fill("另一家完全不同企业");
    await expect(dialog.getByText(/可能重复的/)).toHaveCount(0);
    await name.fill("独立公司");
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: `＋ 新增${label}`, exact: true }).click();
    const create = page.getByRole("dialog", { name: `新增${label}`, exact: true });
    await create.getByRole("textbox", { name: "名称 *", exact: true }).fill("abc公司");
    await expect(create.getByText(`发现 1 条可能重复的${label}`)).toBeVisible();
    await expect(create.getByRole("button", { name: "确认新增" })).toBeEnabled();
    expect(writes).toBe(0);
  });
}
