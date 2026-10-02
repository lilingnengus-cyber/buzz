import { expect, test } from "@playwright/test";

test("商品新建逐项定位必填字段，补齐后保存", async ({ page }) => {
  const writes: unknown[] = [];
  await page.route("**/api/**", async (route) => {
    const req = route.request(), path = new URL(req.url()).pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf", displayName: "测试" } });
    if (req.method() === "POST") { writes.push(req.postDataJSON()); return route.fulfill({ json: { id: "created", version: 1 } }); }
    if (path === "/api/v1/product-master-data") return route.fulfill({ json: {
      items: [{ id: "cat", resourceType: "product_category", code: "CAT", name: "测试分类", status: "active" }, { id: "unit", resourceType: "unit_of_measure", code: "EA", name: "件", status: "active" }], canManage: true, dataAsOf: "2026-10-03T00:00:00Z",
    } });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#productData");
  await page.getByRole("button", { name: /新增商品/ }).click();
  const dialog = page.getByRole("dialog", { name: "新增商品", exact: true });
  const save = dialog.getByRole("button", { name: "确认新增", exact: true });
  const code = dialog.getByRole("textbox", { name: "编码 *", exact: true });
  const name = dialog.getByRole("textbox", { name: "名称 *", exact: true });
  await save.click();
  await expect(code).toBeFocused();
  await expect(dialog.getByRole("alert")).toContainText("请选择商品分类");
  await expect(dialog.getByRole("alert")).toContainText("请选择基础单位");
  await dialog.getByRole("button", { name: "请选择基础单位", exact: true }).click();
  await expect(dialog.getByRole("combobox", { name: /^基础单位/ })).toBeFocused();
  expect(writes).toHaveLength(0);
  await code.fill("NEW-PRODUCT");
  await name.fill("新商品");
  await dialog.getByRole("combobox", { name: /^商品分类/ }).selectOption("cat");
  await dialog.getByRole("combobox", { name: /^基础单位/ }).selectOption("unit");
  await save.click();
  await expect(dialog).toHaveCount(0);
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ name: "新商品", categoryId: "cat", baseUomId: "unit" });
});
