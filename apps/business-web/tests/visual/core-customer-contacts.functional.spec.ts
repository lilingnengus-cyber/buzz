import { expect, test } from "@playwright/test";

test("核心客户按正式 ID 展示分页联系人，详情关闭保留客户编辑", async ({ page }) => {
  const offsets: string[] = [];
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    let json: unknown = { items: [] };
    if (path === "/api/session") json = { authenticated: true, subject: "test", displayName: "测试", csrfToken: "csrf" };
    if (path === "/api/v1/core-master-data") json = { items: [{ id: "customer", resourceType: "customer", code: "CU-001", name: "示例客户", updatedAt: "2026-10-02T00:00:00Z", status: "active", version: 1, creditCurrency: "CNY", creditLimitMinor: 0, paymentTermsDays: 30 }], canManage: true, dataAsOf: "2026-10-02T00:00:00Z" };
    if (path === "/api/v1/crm/accounts") json = { items: [{ id: "other", customerId: "other", name: "示例客户" }, { id: "account", customerId: "customer", name: "示例客户" }] };
    if (path === "/api/v1/crm/contacts") {
      expect(url.searchParams.get("accountId")).toBe("account");
      const offset = url.searchParams.get("offset")!;
      offsets.push(offset);
      json = { items: [{ id: offset, accountId: "account", customerId: "customer", companyName: "示例客户", contactName: offset === "0" ? "张经理" : "李经理", contactDetails: "13800000000", opportunities: [] }], hasMore: offset === "0" };
    }
    await route.fulfill({ json });
  });
  await page.goto("/customers/customer");
  await page.getByRole("article", { name: "查看示例客户详情" }).click();
  const customer = page.getByRole("dialog", { name: "客户详情", exact: true });
  await customer.getByRole("textbox", { name: "名称 *" }).fill("待保存客户名称");
  await customer.getByRole("button", { name: "李经理 · 13800000000" }).click();
  const contact = page.getByRole("dialog", { name: "联系人详情", exact: true });
  await expect(contact.getByRole("heading", { name: "李经理" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(contact).toHaveCount(0);
  await expect(customer.getByRole("textbox", { name: "名称 *" })).toHaveValue("待保存客户名称");
  expect(offsets).toEqual(["0", "50"]);
});

test("关联联系人读取失败可重试，空客户不显示其他联系人", async ({ page }) => {
  let fail = true;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/crm/accounts" && fail) {
      await route.fulfill({ status: 403, json: { message: "没有 CRM 读取权限" } });
      return;
    }
    let json: unknown = { items: [] };
    if (path === "/api/session") json = { authenticated: true, subject: "test", displayName: "测试", csrfToken: "csrf" };
    if (path === "/api/v1/core-master-data") json = { items: [{ id: "customer", resourceType: "customer", code: "CU-001", name: "示例客户", updatedAt: "2026-10-02T00:00:00Z", status: "active", version: 1 }], canManage: false, dataAsOf: "2026-10-02T00:00:00Z" };
    await route.fulfill({ json });
  });
  await page.goto("/customers/customer");
  await page.getByRole("article", { name: "查看示例客户详情" }).click();
  const related = page.getByRole("region", { name: "关联联系人" });
  await expect(related.getByRole("alert")).toBeVisible();
  fail = false;
  await related.getByRole("button", { name: "重试" }).click();
  await expect(related.getByText("尚未关联联系人")).toBeVisible();
});
