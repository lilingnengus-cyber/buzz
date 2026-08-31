import { expect, test } from "@playwright/test";

test("编码规则无权访问时保留工作台框架并可重新检查", async ({ page }) => {
  let accessGranted = false;

  await page.route("**/api/v1/numbering-rules", async (route) => {
    if (!accessGranted) {
      await route.fulfill({
        status: 404,
        json: {
          code: "not_found_or_forbidden",
          message: "resource was not found or is not accessible",
          traceId: "numbering-access-test",
        },
      });
      return;
    }

    await route.fulfill({
      json: { items: [], canManage: false },
    });
  });

  await page.goto("/#numbering");

  await expect(
    page.getByRole("heading", { name: "编码规则", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("基础资料", { exact: true })).toBeVisible();
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("当前账号无法访问编码规则");
  await expect(alert).toContainText("请联系企业管理员确认权限");
  await expect(page.getByText("规则总数", { exact: true })).toBeHidden();

  accessGranted = true;
  await alert.getByRole("button", { name: "重新检查" }).click();

  await expect(alert).toBeHidden();
  await expect(page.getByText("规则总数", { exact: true })).toBeVisible();
});

const accessPages = [
  {
    hash: "coreData",
    resource: "核心数据",
    hiddenActions: ["＋ 新增法定主体"],
  },
  { hash: "productData", resource: "商品主数据", hiddenActions: [] },
  { hash: "inventory", resource: "库存台账", hiddenActions: [] },
  {
    hash: "sales",
    resource: "销售订单",
    hiddenActions: ["新增销售订单", "新建出库单"],
  },
  {
    hash: "purchasing",
    resource: "采购订单",
    hiddenActions: ["新增采购订单", "新建收货单"],
  },
  { hash: "trends", resource: "日报与趋势", hiddenActions: [] },
  {
    hash: "profits",
    resource: "订单真实利润",
    hiddenActions: [],
  },
  {
    hash: "profitability",
    resource: "多维盈利分析",
    hiddenActions: [],
  },
  {
    hash: "adjustments",
    resource: "经营费用归集",
    hiddenActions: ["创建经营调整草稿"],
  },
  {
    hash: "reports",
    resource: "管理利润报表",
    hiddenActions: ["生成管理报表快照"],
  },
] as const;

for (const pageCase of accessPages) {
  test(`${pageCase.resource}使用统一的页面级无权限状态`, async ({ page }) => {
    await page.route("**/api/v1/**", (route) =>
      route.fulfill({
        status: 404,
        json: {
          code: "not_found_or_forbidden",
          message: "resource was not found or is not accessible",
          traceId: `${pageCase.hash}-access-test`,
        },
      }),
    );

    await page.goto(`/#${pageCase.hash}`);

    await expect(page.getByText("基础资料", { exact: true })).toBeVisible();
    const alert = page.getByRole("alert");
    await expect(alert).toHaveAttribute("data-failure-kind", "access_denied");
    await expect(alert).toContainText(`当前账号无法访问${pageCase.resource}`);
    await expect(alert.getByRole("button", { name: "重新检查" })).toBeVisible();
    for (const action of pageCase.hiddenActions) {
      await expect(page.getByText(action, { exact: true })).toBeHidden();
    }
  });
}

test("通用数据页区分服务异常与登录失效", async ({ page }) => {
  let responseStatus = 503;
  await page.route("**/api/v1/operations/dashboard**", (route) =>
    route.fulfill({
      status: responseStatus,
      json:
        responseStatus === 401
          ? { code: "session_expired", message: "session expired" }
          : { code: "service_unavailable", message: "service unavailable" },
    }),
  );
  await page.route("**/api/v1/agent-query-runs", (route) =>
    route.fulfill({ json: { items: [] } }),
  );

  await page.goto("/#dashboard");
  let alert = page.getByRole("alert");
  await expect(alert).toHaveAttribute(
    "data-failure-kind",
    "service_unavailable",
  );
  await expect(alert).toContainText("经营驾驶舱暂时不可用");
  await expect(alert.getByRole("button", { name: "重新加载" })).toBeVisible();

  responseStatus = 401;
  await alert.getByRole("button", { name: "重新加载" }).click();
  alert = page.getByRole("alert");
  await expect(alert).toHaveAttribute("data-failure-kind", "session_expired");
  await expect(alert).toContainText("登录状态已失效");
  await expect(alert.getByRole("button", { name: "重新登录" })).toBeVisible();
});
