import { expect, test } from "@playwright/test";

const id = "54a738b6-49ad-4c5b-9a08-6a16a0a119e2";
const detail = {
  id,
  countNumber: "COUNT-ACCEPTANCE",
  legalEntityId: id,
  warehouseId: id,
  countDate: "2026-09-20",
  currency: "CNY",
  status: "counted",
  version: 2,
  lines: [
    {
      id: "line-1",
      skuId: id,
      skuCode: "SKU-1",
      skuName: "<script>验收商品</script>",
      snapshotOnHandQuantity: "2",
      snapshotReservedQuantity: "0",
      snapshotQuarantinedQuantity: "0",
      actualOnHandQuantity: "0",
      varianceQuantity: "-2",
      varianceValue: "-14",
    },
  ],
};

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

for (const prefix of ["", "/embed"]) {
  test(`${prefix || "standalone"} opens only the selected count without writes`, async ({
    page,
  }) => {
    await page.route("**/api/v1/**", (route) => {
      expect(new URL(route.request().url()).pathname).toBe(
        `/api/v1/inventory-counts/${id}`,
      );
      expect(route.request().method()).toBe("GET");
      return route.fulfill({ json: detail });
    });
    await page.goto(`${prefix}/inventory-counts/${id}`);
    await expect(
      page.getByRole("heading", { name: "库存盘点 · COUNT-ACCEPTANCE" }),
    ).toBeVisible();
    await expect(page.getByRole("table")).toContainText(
      "SKU-1 · <script>验收商品</script>",
    );
    await expect(page.getByRole("table")).toContainText("-14.00");
    await expect(
      page.getByRole("cell", { name: "0.00", exact: true }),
    ).toHaveCount(3);
    await expect(
      page.getByText("所选仓库与商品仍在冻结中", { exact: false }),
    ).toBeVisible();
  });
}

for (const status of ["counting", "posted", "cancelled"]) {
  test(`${status} shows accurate freeze and missing quantity state`, async ({
    page,
  }) => {
    await page.route("**/api/v1/**", (route) =>
      route.fulfill({
        json: {
          ...detail,
          status,
          lines: detail.lines.map((line) => ({
            ...line,
            actualOnHandQuantity: null,
            varianceQuantity: null,
            varianceValue: null,
          })),
        },
      }),
    );
    await page.goto(`/embed/inventory-counts/${id}`);
    await expect(
      page.getByRole("cell", { name: "未录入", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText(
        status === "counting"
          ? "所选仓库与商品仍在冻结中"
          : "当前盘点的冻结已解除",
        { exact: false },
      ),
    ).toBeVisible();
  });
}

test("navigation to denied count removes the previous detail", async ({
  page,
}) => {
  await page.route("**/api/v1/**", (route) =>
    route.fulfill(
      new URL(route.request().url()).pathname.endsWith(id)
        ? { json: detail }
        : { status: 404, json: { code: "not_found_or_forbidden" } },
    ),
  );
  await page.goto(`/embed/inventory-counts/${id}`);
  await expect(page.getByRole("table")).toBeVisible();
  await page.evaluate(() => {
    history.pushState({}, "", "/embed/inventory-counts/unavailable");
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await expect(
    page.getByRole("heading", { name: "当前账号无法访问库存盘点" }),
  ).toBeVisible();
  await expect(page.getByRole("table")).toHaveCount(0);
  await expect(
    page.getByText("COUNT-ACCEPTANCE", { exact: false }),
  ).toHaveCount(0);
});
