import { test, expect } from "@playwright/test";

test("重复、必填、日期和列数错误逐行提示，修正后重新预览", async ({ page }) => {
  const writes: string[] = [];
  await page.route("**/api/**", (route) => {
    if (route.request().method() === "POST") writes.push(route.request().url());
    return route.fulfill({
      json:
        new URL(route.request().url()).pathname === "/api/session"
          ? { authenticated: true, csrfToken: "csrf" }
          : { items: [], hasMore: false, canManage: true },
    });
  });
  await page.goto("/#crmLeads");
  await page.getByRole("button", { name: "批量导入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "批量导入线索" });
  const source = dialog.getByLabel("表格内容");
  await source.fill(
    "线索名称,客户公司,跟进日期\n项目甲,公司甲,2026-10-10\n项目甲,公司甲,2026-10-10\n,公司乙,\n项目丙,公司丙,2026-02-30\n项目丁,公司丁",
  );
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("status")).toContainText(
    "共 5 条，校验错误 4 条",
  );
  for (const [row, error] of [
    [3, "与本批前面记录完全重复"],
    [4, "线索名称必填"],
    [5, "跟进日期须为有效的 YYYY-MM-DD 日期"],
    [6, "列数与表头不一致"],
  ] as const) {
    const record = dialog.locator("tbody tr").filter({
      has: page.getByRole("cell", { name: String(row), exact: true }),
    });
    await expect(record).toContainText(error);
  }
  await expect(dialog.getByRole("button", { name: "确认导入" })).toBeDisabled();
  await source.fill(
    "线索名称,客户公司,跟进日期\n项目甲,公司甲,2026-10-10\n项目甲,公司乙,2026-10-11",
  );
  await expect(dialog.locator("tbody")).toHaveCount(0);
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("status")).toContainText(
    "共 2 条，校验错误 0 条",
  );
  await expect(dialog.getByRole("button", { name: "确认导入" })).toBeEnabled();
  await source.fill("线索名称,线索名称\n甲,乙");
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("alert")).toContainText("表头重复或不受支持");
  await expect(dialog.getByRole("button", { name: "确认导入" })).toHaveCount(0);
  expect(writes).toHaveLength(0);
});

test("关闭后重复导入相同内容复用幂等标识", async ({ page }) => {
  const keys: string[] = [];
  const records = new Map<string, string>();
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/crm/leads" && route.request().method() === "POST") {
      const key = route.request().headers()["idempotency-key"];
      keys.push(key);
      if (!records.has(key)) records.set(key, "imported-lead");
      return route.fulfill({ json: { id: records.get(key), version: 1 } });
    }
    return route.fulfill({
      json:
        path === "/api/session"
          ? { authenticated: true, csrfToken: "csrf" }
          : { items: [], hasMore: false, canManage: true },
    });
  });
  await page.goto("/#crmLeads");
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.getByRole("button", { name: "批量导入", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "批量导入线索" });
    await dialog
      .getByLabel("表格内容")
      .fill("线索名称,客户公司\n验收项目,验收公司");
    await dialog.getByRole("button", { name: "预览校验" }).click();
    await dialog.getByRole("button", { name: "确认导入" }).click();
    await expect(dialog.getByRole("status")).toContainText("已保存 1 条");
    await expect(dialog.locator("tbody")).toContainText("重复提交不会新建");
    await page.keyboard.press("Escape");
    await expect(dialog).not.toBeVisible();
  }
  expect(keys).toHaveLength(2);
  expect(keys[0]).toBe(keys[1]);
  expect(records.size).toBe(1);
});

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
      '\uFEFF线索名称,客户公司,联系人,联系方式,来源,需求摘要,下一步,跟进日期\r\n"项目,甲",客户甲,张经理,13800138000,展会,年度采购需求,联系客户,2026-10-10',
    ),
  });
  await expect(dialog.getByLabel("表格内容")).toContainText("项目,甲");
  await dialog.getByRole("button", { name: "预览校验" }).click();
  await expect(dialog.getByRole("status")).toContainText("校验错误 0 条");
  await expect(dialog.locator("tbody")).toContainText("联系客户");
  for (const column of [
    "线索名称",
    "公司名称",
    "联系人",
    "联系方式",
    "来源",
    "需求摘要",
    "下一步",
    "跟进日期",
  ]) {
    await expect(
      dialog.getByRole("columnheader", { name: column, exact: true }),
    ).toBeVisible();
  }
  const cells = dialog.locator("tbody tr").first().getByRole("cell");
  await expect(cells.nth(1)).toHaveText("项目,甲");
  await expect(cells.nth(2)).toHaveText("客户甲");
  await expect(cells.nth(3)).toHaveText("张经理");
  await expect(cells.nth(4)).toHaveText("13800138000");
  await expect(cells.nth(5)).toHaveText("展会");
  await expect(cells.nth(6)).toHaveText("年度采购需求");
  await expect(cells.nth(7)).toHaveText("联系客户");
  await expect(cells.nth(8)).toHaveText("2026-10-10");
  await page.setViewportSize({ width: 520, height: 780 });
  expect(
    await dialog.evaluate((el) => el.getBoundingClientRect().right),
  ).toBeLessThanOrEqual(520);
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(520);
  await expect(cells.nth(1)).toHaveText("项目,甲");
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
