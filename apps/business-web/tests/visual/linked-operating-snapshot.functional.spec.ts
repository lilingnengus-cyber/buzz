import { expect, test } from "@playwright/test";
test.beforeEach(async ({ page }) => {
  await page.route("**/api/session", (route) =>
    route.fulfill({
      json: {
        authenticated: true,
        subject: "fixture",
        displayName: "验收用户",
      },
    }),
  );
});
for (const cadence of ["daily", "weekly"]) {
  test(`${cadence}快照链接读取指定冻结报表并支持权限重试`, async ({ page }) => {
    const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
    let allowed = false;
    const requests: string[] = [];
    await page.route("**/api/v1/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      requests.push(path);
      if (path !== `/api/v1/operations/snapshots/${id}`) {
        await route.fulfill({ status: 500, json: {} });
        return;
      }
      if (!allowed) {
        await route.fulfill({
          status: 404,
          json: { code: "not_found_or_forbidden", message: "not accessible" },
        });
        return;
      }
      await route.fulfill({
        json: {
          id,
          cadence,
          periodStart: "2026-01-19",
          periodEnd: cadence === "daily" ? "2026-01-20" : "2026-01-26",
          currency: "CNY",
          utcOffsetMinutes: 480,
          dataQualityStatus: "complete",
          generatedAt: "2026-02-01T00:00:00Z",
          sourceHash: "a".repeat(64),
          scope: { legalEntityIds: [id] },
          scopeBasis: "recorded",
          ownerUserId: id,
          metrics: {
            salesOrderCount: 2,
            salesOrderAmount: "123.45",
            shipmentCount: 1,
            shippedRevenue: "100.00",
            purchaseOrderCount: 1,
            purchaseOrderAmount: "30.00",
            inventoryValueAsOfGeneration: "600.00",
            stockoutCountAsOfGeneration: 0,
            managementOperatingProfit: "70.00",
            incidentsOpened: 1,
            incidentsResolved: 1,
            slaBreached: 0,
            averageResolutionHours: "2.00",
          },
        },
      });
    });
    await page.goto(`/embed/operating-snapshots/${id}`);
    await expect(page.getByRole("alert")).toHaveAttribute(
      "data-failure-kind",
      "access_denied",
    );
    allowed = true;
    await page.getByRole("button", { name: "重新检查" }).click();
    await expect(
      page.getByRole("heading", {
        name: cadence === "daily" ? "经营日报" : "经营周报",
      }),
    ).toBeVisible();
    await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
      "CNY 123.45",
    );
    await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
      "UTC+08:00",
    );
    await expect(
      page.getByRole("link", { name: "返回日报与趋势" }),
    ).toHaveAttribute("href", "/embed/operating-trends");
    expect(requests).toEqual([
      `/api/v1/operations/snapshots/${id}`,
      `/api/v1/operations/snapshots/${id}`,
    ]);
    await expect(
      page.getByTestId("operating-snapshot-detail").getByRole("button"),
    ).toHaveCount(0);
  });
}

test("法人筛选快照不把无法归属的异常显示为零", async ({ page }) => {
  const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
  await page.route("**/api/v1/operations/snapshots/*", (route) =>
    route.fulfill({
      json: {
        id,
        cadence: "daily",
        periodStart: "2026-03-02",
        periodEnd: "2026-03-03",
        currency: "CNY",
        utcOffsetMinutes: 480,
        dataQualityStatus: "partial",
        generatedAt: "2026-03-04T00:00:00Z",
        sourceHash: "a".repeat(64),
        scope: { legalEntityIds: [id] },
        scopeBasis: "recorded",
        metrics: {
          salesOrderCount: 1,
          salesOrderAmount: "123.45",
          shipmentCount: 1,
          shippedRevenue: "100.00",
          purchaseOrderCount: 0,
          purchaseOrderAmount: "0.00",
          inventoryValueAsOfGeneration: "600.00",
          stockoutCountAsOfGeneration: 0,
          managementOperatingProfit: "70.00",
          incidentsOpened: null,
          incidentsResolved: null,
          slaBreached: null,
          averageResolutionHours: null,
          unavailableMetrics: {
            incidentsOpened: "not_attributable_to_selected_legal_entities",
            incidentsResolved: "not_attributable_to_selected_legal_entities",
            slaBreached: "not_attributable_to_selected_legal_entities",
            averageResolutionHours:
              "not_attributable_to_selected_legal_entities",
          },
        },
      },
    }),
  );
  await page.goto(`/embed/operating-snapshots/${id}`);
  await expect(
    page.getByRole("cell", { name: "不可按法人拆分", exact: true }),
  ).toHaveCount(4);
  await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
    "CNY 123.45",
  );
  await expect(page.getByTestId("operating-snapshot-detail")).not.toContainText(
    "null",
  );
});

test("业务单元筛选快照不把无法归属的异常显示为零", async ({ page }) => {
  const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
  await page.route("**/api/v1/operations/snapshots/*", (route) =>
    route.fulfill({
      json: {
        id,
        cadence: "daily",
        periodStart: "2026-03-02",
        periodEnd: "2026-03-03",
        currency: "CNY",
        utcOffsetMinutes: 480,
        dataQualityStatus: "partial",
        generatedAt: "2026-03-04T00:00:00Z",
        sourceHash: "a".repeat(64),
        scope: { legalEntityIds: [id] },
        scopeBasis: "recorded",
        metrics: {
          salesOrderCount: 1,
          salesOrderAmount: "123.45",
          shipmentCount: 1,
          shippedRevenue: "100.00",
          purchaseOrderCount: 0,
          purchaseOrderAmount: "0.00",
          inventoryValueAsOfGeneration: "600.00",
          stockoutCountAsOfGeneration: 0,
          managementOperatingProfit: "70.00",
          incidentsOpened: null,
          incidentsResolved: null,
          slaBreached: null,
          averageResolutionHours: null,
          unavailableMetrics: {
            incidentsOpened: "not_attributable_to_selected_business_units",
            incidentsResolved: "not_attributable_to_selected_business_units",
            slaBreached: "not_attributable_to_selected_business_units",
            averageResolutionHours:
              "not_attributable_to_selected_business_units",
          },
        },
      },
    }),
  );
  await page.goto(`/embed/operating-snapshots/${id}`);
  await expect(
    page.getByRole("cell", { name: "不可按业务单元拆分", exact: true }),
  ).toHaveCount(4);
  await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
    "CNY 123.45",
  );
  await expect(page.getByTestId("operating-snapshot-detail")).not.toContainText(
    "null",
  );
});

test("仓库筛选快照不把无法归属的异常显示为零", async ({ page }) => {
  const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
  await page.route("**/api/v1/operations/snapshots/*", (route) =>
    route.fulfill({
      json: {
        id,
        cadence: "daily",
        periodStart: "2026-03-02",
        periodEnd: "2026-03-03",
        currency: "CNY",
        utcOffsetMinutes: 480,
        dataQualityStatus: "partial",
        generatedAt: "2026-03-04T00:00:00Z",
        sourceHash: "a".repeat(64),
        scope: { legalEntityIds: [id] },
        scopeBasis: "recorded",
        metrics: {
          aggregationBasis: {
            orderAmounts: "selected_warehouse_lines",
            orderCounts: "distinct_orders_with_selected_warehouse_lines",
            businessUnitFilterApplied: false,
          },
          salesOrderCount: 1,
          salesOrderAmount: "123.45",
          shipmentCount: 1,
          shippedRevenue: "100.00",
          purchaseOrderCount: 0,
          purchaseOrderAmount: "0.00",
          inventoryValueAsOfGeneration: "600.00",
          stockoutCountAsOfGeneration: 0,
          managementOperatingProfit: "70.00",
          incidentsOpened: null,
          incidentsResolved: null,
          slaBreached: null,
          averageResolutionHours: null,
          unavailableMetrics: {
            incidentsOpened: "not_attributable_to_selected_warehouses",
            incidentsResolved: "not_attributable_to_selected_warehouses",
            slaBreached: "not_attributable_to_selected_warehouses",
            averageResolutionHours: "not_attributable_to_selected_warehouses",
          },
        },
      },
    }),
  );
  await page.goto(`/embed/operating-snapshots/${id}`);
  await expect(
    page.getByRole("cell", { name: "不可按仓库拆分", exact: true }),
  ).toHaveCount(4);
  await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
    "CNY 123.45",
  );
  await expect(page.getByTestId("operating-snapshot-detail")).toContainText(
    "订单金额仅含所选仓库的明细",
  );
  await expect(page.getByTestId("operating-snapshot-detail")).not.toContainText(
    "null",
  );
});
