import { expect, test } from "@playwright/test";
const id = "123e4567-e89b-12d3-a456-426614174000";
for (const prefix of ["", "/embed"]) {
  test(`${prefix || "standalone"} opens the linked CRM opportunity without writes`, async ({
    page,
  }) => {
    let detailReads = 0;
    await page.route("**/api/**", (route) => {
      const path = new URL(route.request().url()).pathname;
      expect(route.request().method()).toBe("GET");
      if (path === "/api/session")
        return route.fulfill({
          json: { authenticated: true, displayName: "CRM test" },
        });
      if (path === `/api/v1/crm/opportunities/${id}`) {
        detailReads++;
        return route.fulfill({
          json: {
            item: {
              id,
              title: "链接指定商机",
              companyName: "目标公司",
              contactName: "陈经理",
              contactDetails: "",
              stage: "quoting",
              expectedAmountMinor: 20000,
              currency: "CNY",
              nextAction: "确认报价",
              nextFollowUp: null,
              version: 2,
            },
            followups: [],
            hasOlderFollowups: false,
          },
        });
      }
      return route.fulfill({
        json: { items: [], canManage: false, hasMore: false },
      });
    });
    await page.goto(`${prefix}/crm/opportunities/${id}`);
    const detail = page.getByRole("complementary", { name: "商机详情" });
    await expect(
      detail.getByRole("heading", { name: "链接指定商机" }),
    ).toBeVisible();
    await expect(detail).toContainText("目标公司");
    expect(detailReads).toBe(1);
  });
}
