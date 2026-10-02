import { expect, test } from "@playwright/test";

for (const [resourceType, title] of [["customer", "客户"], ["supplier", "供应商"], ["warehouse", "仓库"], ["product", "商品"], ["sku", "SKU / 条码"]]) {
  test(`${title}修改关闭保护、还原及保存失败保留`, async ({ page }) => {
    const product = ["product", "sku"].includes(resourceType);
    const endpoint = product ? "/api/v1/product-master-data" : "/api/v1/core-master-data";
    let fail = true;
    const writes: unknown[] = [];
    await page.route("**/api/**", async (route) => {
      const request = route.request(), path = new URL(request.url()).pathname;
      if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf", displayName: "测试" } });
      if (request.method() === "PUT") {
        writes.push(request.postDataJSON());
        return fail ? route.fulfill({ status: 409, json: { error: "版本冲突，请重试" } }) : route.fulfill({ json: { id: "record", version: 2 } });
      }
      if (path === endpoint) return route.fulfill({ json: {
        items: [{ id: "record", resourceType, categoryId: "category", productId: "product", unitOfMeasureId: "uom", code: "TEST-01", name: `示例${title}`, status: "active", version: 1, updatedAt: "2026-10-03T00:00:00Z" }],
        canManage: true, dataAsOf: "2026-10-03T00:00:00Z",
      } });
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto(product ? "/#productData" : "/#coreData");
    await page.getByRole("tab", { name: resourceType === "product" ? /^SPU 商品 / : title, exact: false }).click();
    const row = page.getByRole("article", { name: `查看示例${title}详情`, exact: true });
    await row.click();
    const dialog = page.getByRole("dialog", { name: `${title}详情`, exact: true });
    const name = dialog.getByRole("textbox", { name: "名称 *", exact: true });
    await name.fill("修改内容");
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
    const prompt = page.getByRole("dialog", { name: "放弃未保存修改", exact: true });
    await expect(prompt).toBeVisible();
    await prompt.getByRole("button", { name: "继续编辑", exact: true }).click();
    await expect(name).toHaveValue("修改内容");
    await dialog.getByRole("button", { name: "保存修订", exact: true }).click();
    await expect(dialog.locator(".master-form-error")).toBeVisible();
    await expect(name).toHaveValue("修改内容");
    await page.keyboard.press("Escape");
    await expect(prompt).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(prompt).toHaveCount(0);
    await name.fill(`示例${title}`);
    await dialog.getByRole("button", { name: "关闭弹窗", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await row.click();
    await name.fill("放弃这次修改");
    await page.mouse.click(1, 1);
    await expect(prompt).toBeVisible();
    await prompt.getByRole("button", { name: "放弃修改", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect(writes).toHaveLength(1);
    await row.click();
    await name.fill("保存这次修改");
    fail = false;
    await dialog.getByRole("button", { name: "保存修订", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(prompt).toHaveCount(0);
    expect(writes).toHaveLength(2);
  });
}
