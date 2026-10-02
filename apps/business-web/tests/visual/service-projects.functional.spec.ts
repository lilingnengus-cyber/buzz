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
