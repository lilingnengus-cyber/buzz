import { test, expect } from "@playwright/test";
test("批量导入先校验、部分失败重试复用标识、成功项不重发", async ({ page }) => {
  const writes: any[] = [];
  let fail = true;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf" },
      });
    if (path === "/api/v1/crm/options")
      return route.fulfill({
        json: {
          items: [
            {
              id: "le",
              name: "法人",
              resourceType: "legal_entity",
              status: "active",
            },
            {
              id: "bu",
              name: "经营单元",
              resourceType: "business_unit",
              status: "active",
              ancestorPath: ["经营单元"],
            },
          ],
        },
      });
    if (path === "/api/v1/crm/leads" && route.request().method() === "POST") {
      const body = route.request().postDataJSON();
      writes.push({ body, key: route.request().headers()["idempotency-key"] });
      if (body.title === "项目乙" && fail) {
        fail = false;
        return route.fulfill({ status: 503, json: { message: "暂时不可用" } });
      }
      return route.fulfill({
        json: { id: body.title === "项目甲" ? "a" : "b", version: 1 },
      });
    }
    return route.fulfill({
      json: { items: [], hasMore: false, canManage: true },
    });
  });
  await page.goto("/#crm");
  await expect(
    page.getByRole("button", { name: "批量导入", exact: true }),
  ).toHaveCount(0);
  await page.goto("/#crmLeads");
  await page.getByRole("button", { name: "批量导入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "批量导入线索" });
  await dialog
    .getByLabel("表格内容")
    .fill("线索名称,客户公司\n项目甲,客户甲\n,客户乙");
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("button", { name: "确认导入" })).toBeDisabled();
  expect(writes).toHaveLength(0);
  await dialog
    .getByLabel("表格内容")
    .fill("线索名称,客户公司\n项目甲,客户甲\n项目乙,客户乙");
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("status")).toContainText("共 2 条");
  expect(writes).toHaveLength(0);
  await dialog.getByRole("button", { name: "确认导入" }).click();
  await expect(
    dialog.getByRole("button", { name: "重试未成功记录" }),
  ).toBeEnabled();
  await expect(dialog.getByRole("status")).toContainText("已保存 1 条");
  await dialog.getByRole("button", { name: "重试未成功记录" }).click();
  await expect(dialog.getByRole("status")).toContainText("已保存 2 条");
  expect(writes).toHaveLength(3);
  expect(writes[1].key).toBe(writes[2].key);
  expect(writes[0].body.title).toBe("项目甲");
  expect(writes[0].body).not.toHaveProperty("legalEntityId");
  expect(writes[0].body).not.toHaveProperty("stage");
  expect(writes[0].key).toMatch(/^crm-lead-import-v1-/);
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
});

test("CSV 模板下载、上传预览与未保存保护", async ({ page }) => {
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({
      json:
        path === "/api/session"
          ? { authenticated: true, csrfToken: "csrf" }
          : path === "/api/v1/crm/options"
            ? {
                items: [
                  {
                    id: "le",
                    name: "法人",
                    resourceType: "legal_entity",
                    status: "active",
                  },
                  {
                    id: "bu",
                    name: "经营单元",
                    resourceType: "business_unit",
                    status: "active",
                    ancestorPath: ["经营单元"],
                  },
                ],
              }
            : { items: [], hasMore: false, canManage: true },
    });
  });
  await page.goto("/#crm");
  await expect(
    page.getByRole("button", { name: "批量导入", exact: true }),
  ).toHaveCount(0);
  await page.goto("/#crmLeads");
  await page.getByRole("button", { name: "批量导入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "批量导入线索" });
  const pending = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "下载 CSV 模板" }).click();
  expect((await pending).suggestedFilename()).toBe("线索导入模板.csv");
  await dialog.getByLabel("上传 CSV").setInputFiles({
    name: "opportunities.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(
      '\uFEFF线索名称,客户公司,联系人,联系方式,下一步,跟进日期\r\n"项目,甲",客户甲,张经理,13800138000,联系客户,2026-10-10',
    ),
  });
  await expect(dialog.getByLabel("表格内容")).toContainText("项目,甲");
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("status")).toContainText("校验错误 0 条");
  await expect(dialog.locator("tbody")).toContainText("联系客户");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "继续编辑" }).click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "放弃修改", exact: true }).click();
  await expect(dialog).not.toBeVisible();
});

test("只读线索页隐藏导入入口", async ({ page }) => {
  await page.route("**/api/**", (route) =>
    route.fulfill({
      json:
        new URL(route.request().url()).pathname === "/api/session"
          ? { authenticated: true, csrfToken: "csrf" }
          : { items: [], hasMore: false, canManage: false },
    }),
  );
  await page.goto("/#crmLeads");
  await expect(
    page.getByRole("heading", { name: "线索", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "批量导入", exact: true }),
  ).toHaveCount(0);
});
