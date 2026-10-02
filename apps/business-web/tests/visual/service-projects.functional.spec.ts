import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";
for (const canManage of [true, false]) {
  test(`服务交付详情、验收及只读权限 canManage=${canManage}`, async ({
    page,
  }) => {
    let item = {
      id: "project",
      version: 3,
      title: "软件实施",
      status: "acceptance",
      legal_entity_id: "legal",
      business_unit_id: "unit",
      customer_id: "customer",
      owner_user_id: "actor",
      customer_name: "测试客户",
      service_kind: "software_service",
      owner_name: "负责人",
      sales_order_line_id: "line",
      starts_on: "2026-10-01",
      ends_on: "2027-09-30",
    };
    const writes: unknown[] = [],
      acceptances: any[] = [];
    await page.route("**/api/**", async (route) => {
      const req = route.request(),
        url = new URL(req.url());
      if (url.pathname === "/api/session")
        return route.fulfill({
          json: { authenticated: true, csrfToken: "csrf", displayName: "测试" },
        });
      if (url.pathname === "/api/v1/service-projects/project/acceptances") {
        const input = req.postDataJSON();
        writes.push(input);
        item = { ...item, status: "completed", version: 4 };
        acceptances.push({
          id: "acceptance",
          accepted_on: input.acceptedOn,
          customer_reviewer: input.customerReviewer,
          result: input.result,
          note: input.note,
          evidence_url: input.evidenceUrl,
        });
        return route.fulfill({ json: { id: "acceptance", version: 4 } });
      }
      if (url.pathname === "/api/v1/service-projects/project")
        return route.fulfill({
          json: {
            item,
            canAccept: canManage,
            receivable: acceptances.length
              ? {
                  number: "AR-1",
                  amount: "106.00",
                  openAmount: "106.00",
                  currency: "CNY",
                  dueDate: "2026-11-01",
                }
              : null,
            tasks: [],
            acceptances,
            hasMoreTasks: false,
            hasMoreAcceptances: false,
          },
        });
      if (url.pathname === "/api/v1/service-projects")
        return route.fulfill({
          json: { items: [item], hasMore: false, canManage },
        });
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto("/#serviceProjects");
    await expect(
      page.getByRole("heading", { name: "服务项目", exact: true }),
    ).toBeVisible();
    const filtered = page.waitForRequest(
      (r) =>
        r.url().includes("/api/v1/service-projects?") &&
        r.url().includes("expiry=expired") &&
        r.url().includes("today="),
    );
    await page.getByLabel("软件服务到期").selectOption("expired");
    await filtered;
    await page.getByRole("button", { name: /软件实施/ }).click();
    const drawer = page.getByRole("dialog", { name: "服务项目详情" });
    await expect(
      drawer.getByRole("heading", { name: "软件实施" }),
    ).toBeVisible();
    if (!canManage) {
      await expect(
        drawer.getByRole("button", { name: "记录验收结果" }),
      ).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect(drawer).not.toBeVisible();
      return;
    }
    await drawer.getByRole("button", { name: "记录验收结果" }).click();
    await drawer.getByLabel("客户验收人").fill("张经理");
    await drawer.getByLabel("验收说明").fill("已核对交付内容");
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "继续编辑" }).click();
    await expect(drawer.getByLabel("验收说明")).toHaveValue("已核对交付内容");
    await drawer.getByRole("button", { name: "保存验收结果" }).click();
    await expect(
      drawer.getByRole("heading", { name: "验收记录" }),
    ).toBeVisible();
    await expect(drawer.getByText("已核对交付内容")).toBeVisible();
    await expect(drawer.getByRole("status")).toContainText("应收 AR-1");
    expect(writes).toHaveLength(1);
    expect(writes[0]).toMatchObject({
      expectedVersion: 3,
      result: "passed",
      customerReviewer: "张经理",
    });
    await expect(drawer.getByRole("button", { name: "编辑项目" })).toHaveCount(
      0,
    );
    await waitForAnimations(page);
    await page.screenshot({
      path: "test-results/service-project-accepted.png",
    });
    await page.keyboard.press("Escape");
    await expect(drawer).not.toBeVisible();
  });
}

test("从订单创建服务项目，回填客户主体并保存来源", async ({ page }) => {
  const line = {
    id: "line",
    order_number: "SO-100",
    title: "年度软件",
    legal_entity_id: "legal",
    business_unit_id: "unit",
    customer_id: "customer",
    customer_name: "客户",
    service_kind: "software_service",
    amount: "106",
    currency: "CNY",
  };
  let saved: any;
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      url = new URL(req.url());
    if (url.pathname === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf", displayName: "测试" },
      });
    if (url.pathname === "/api/v1/service-project-options")
      return route.fulfill({
        json: {
          items: [
            ["legal_entity", "legal", "法人"],
            ["business_unit", "unit", "经营单元"],
            ["customer", "customer", "客户"],
          ].map(([resourceType, id, name]) => ({
            resourceType,
            id,
            name,
            code: id,
            status: "active",
          })),
          owners: [{ id: "actor", name: "负责人" }],
          currentUserId: "actor",
          orderLines: [line],
        },
      });
    if (
      url.pathname === "/api/v1/service-projects" &&
      req.method() === "POST"
    ) {
      saved = req.postDataJSON();
      return route.fulfill({ json: { id: "project", version: 1 } });
    }
    return route.fulfill({
      json: { items: [], canManage: true, hasMore: false },
    });
  });
  await page.goto("/#serviceProjects?order=SO-100");
  const drawer = page.getByRole("dialog");
  await drawer
    .getByRole("combobox", { name: "关联已确认服务订单" })
    .selectOption("line");
  await expect(drawer.getByLabel("名称", { exact: true })).toHaveValue(
    "年度软件",
  );
  await expect(drawer.getByRole("combobox", { name: "法定主体", exact: true })).toBeDisabled();
  await drawer.getByLabel("开始日期").fill("2026-10-01");
  await drawer.getByLabel("结束日期").fill("2027-09-30");
  await drawer.getByRole("button", { name: "保存", exact: true }).click();
  await expect
    .poll(() => saved)
    .toMatchObject({
      salesOrderLineId: "line",
      legalEntityId: "legal",
      businessUnitId: "unit",
      customerId: "customer",
      serviceKind: "software_service",
    });
  await expect(drawer).not.toBeVisible();
});
