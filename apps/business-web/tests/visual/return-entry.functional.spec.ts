import { expect, test } from "@playwright/test";

for (const side of ["sales", "purchase"]) {
  test(`${side} 退货遵守来源权限并校验可退数量`, async ({ page }) => {
    let allowed = false;
    const writes: unknown[] = [];
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "test", displayName: "验收" } });
      if (path === `/api/v1/${side}-returns/options`) return route.fulfill({ json: { canCreate: allowed, items: [{ sourceId: "source", sourceNumber: "SOURCE-001", sourceLineId: "line", orderNumber: "ORDER-001", partnerName: "往来单位", warehouseName: "测试仓库", skuCode: "SKU-TEST", skuName: "测试商品", sourceQuantity: "10", returnedQuantity: "2", returnableQuantity: "8" }] } });
      if (path === `/api/v1/${side}-returns` && route.request().method() === "POST") {
        writes.push(route.request().postDataJSON());
        return route.fulfill({ json: { id: "return", status: "draft", version: 1 } });
      }
      return route.fulfill({ json: { items: [], hasMore: false, canManage: true } });
    });
    const label = side === "sales" ? "销售" : "采购";
    await page.goto("/#sales");
    await page.getByRole("navigation", { name: "业务导航" }).getByRole("link", { name: side === "sales" ? "商品订单闭环" : "采购订单闭环", exact: true }).click();
    await page.getByRole("navigation", { name: "订单闭环阶段" }).getByRole("button", { name: new RegExp(`${label}退货`) }).click();
    const open = page.getByRole("button", { name: new RegExp(`新增${label}退货$`) });
    await open.click();
    const save = page.getByRole("button", { name: `保存${label}退货单草稿` });
    await expect(save).toBeDisabled();
    await expect(page.getByRole("alert")).toContainText("没有");
    // Reload the entry with a permitted options response.
    allowed = true;
    await page.reload();
    await page.getByRole("navigation", { name: "订单闭环阶段" }).getByRole("button", { name: new RegExp(`${label}退货`) }).click();
    await open.click();
    await expect(save).toBeEnabled();
    await page.getByLabel("SKU-TEST 退货数量").fill("9");
    await save.click();
    await expect(page.getByRole("alert")).toContainText("不能超过");
    expect(writes).toHaveLength(0);
    await page.getByLabel("SKU-TEST 退货数量").fill("2");
    await save.click();
    await expect.poll(() => writes.length).toBe(1);
    expect(writes[0]).toMatchObject({ sourceId: "source", lines: [{ sourceLineId: "line", quantity: "2" }] });
  });
}
