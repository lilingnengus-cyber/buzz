import { expect, test } from "@playwright/test";
const id = "123e4567-e89b-12d3-a456-426614174000";
for (const kind of [
  "legal_entity",
  "business_unit",
  "customer",
  "supplier",
  "warehouse",
  "unit_of_measure",
  "product_category",
  "brand",
  "product",
  "sku",
  "uom_conversion",
]) {
  test(`master detail ${kind} loads exactly the linked record`, async ({
    page,
  }) => {
    let reads = 0;
    await page.route("**/api/**", (route) => {
      expect(route.request().method()).toBe("GET");
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/session")
        return route.fulfill({
          json: { authenticated: true, displayName: "Master test" },
        });
      if (path.endsWith(`/${kind}/${id}`)) {
        reads++;
        return route.fulfill({
          json: {
            item: {
              resourceType: kind,
              id,
              code: "TEST",
              name: "链接指定资料",
              status: "active",
              version: 2,
              address: kind === "warehouse" ? "杭州仓库地址" : null,
              factorToBase: kind === "uom_conversion" ? "0.33333333" : null,
            },
          },
        });
      }
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto(`/embed/master-data/${kind}/${id}`);
    const detail = page.getByRole("region", { name: "基础资料详情" });
    await expect(detail.getByRole("heading")).toContainText("链接指定资料");
    await expect(detail).toContainText("版本 2");
    if (kind === "warehouse")
      await expect(detail).toContainText("杭州仓库地址");
    if (kind === "uom_conversion")
      await expect(detail).toContainText("0.33333333");
    expect(reads).toBe(1);
  });
}
test("denied master link shows access failure and no record", async ({
  page,
}) => {
  await page.route("**/api/**", (route) =>
    new URL(route.request().url()).pathname === "/api/session"
      ? route.fulfill({ json: { authenticated: true } })
      : route.fulfill({
          status: 404,
          json: { code: "not_found_or_forbidden", message: "not accessible" },
        }),
  );
  await page.goto(`/master-data/customer/${id}`);
  await expect(
    page.getByRole("region", { name: "基础资料详情" }),
  ).toContainText("无法访问");
  await expect(page.getByText("链接指定资料")).toHaveCount(0);
});
