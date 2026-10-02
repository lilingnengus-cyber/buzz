import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

for (const flow of ["sales", "purchase"] as const) {
  test(`${flow} 法人切换保留每行共享仓库及往来方`, async ({ page }) => {
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/session") {
        await route.fulfill({
          json: { authenticated: true, subject: "tester", csrfToken: "test" },
        });
      } else if (path.endsWith("/entry-options")) {
        await route.fulfill({
          json: { canCreate: true, canUpdate: true, draft: null },
        });
      } else if (path.startsWith("/api/v1/master-data/")) {
        const resource = path.split("/").at(-1) ?? "";
        const items = [1, 2].map((index) => ({
          id: `${resource}-${index}`,
          code: `${resource}-${index}`,
          name: `${resource}-${index}`,
          status: "active",
          resourceType: resource,
          legalEntityId: null,
        }));
        await route.fulfill({ json: { items } });
      } else if (path === "/api/v1/core-master-data") {
        await route.fulfill({
          json: {
            items: [
              {
                id: "bu",
                code: "BU",
                name: "经营主体",
                resourceType: "business_unit",
                status: "active",
                parentBusinessUnitId: null,
                ancestorPath: ["经营主体"],
                depth: 0,
                descendantCount: 0,
              },
            ],
          },
        });
      } else {
        await route.fulfill({ json: { items: [] } });
      }
    });
    const purchase = flow === "purchase";
    await page.goto(purchase ? "/#purchasing" : "/#sales");
    const title = purchase ? "新增采购订单" : "新增销售订单";
    await page.getByRole("button", { name: title, exact: true }).click();
    const dialog = page.getByRole("dialog", { name: title });
    const party = dialog.getByLabel(purchase ? "供应商" : "客户", {
      exact: true,
    });
    await party.click();
    await dialog.getByRole("combobox", { name: purchase ? "搜索供应商" : "搜索客户", exact: true }).fill("search-only");
    await page.keyboard.press("Escape");
    await dialog.getByRole("button", { name: "关闭弹窗", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole("dialog", { name: "放弃未保存修改" })).toHaveCount(0);
    await page.getByRole("button", { name: title, exact: true }).click();
    await party.click();
    const partySearch = dialog.getByRole("combobox", { name: purchase ? "搜索供应商" : "搜索客户", exact: true });
    await partySearch.fill(`${purchase ? "SUPPLIER" : "CUSTOMER"}-2`);
    await partySearch.press("Enter");
    const product = dialog.getByRole("combobox", { name: "第 1 行商品", exact: true });
    await product.click();
    const search = dialog.getByRole("combobox", { name: "搜索第 1 行商品", exact: true });
    await search.fill("missing-product");
    await expect(dialog.getByRole("listbox", { name: "第 1 行商品", exact: true }).getByRole("option")).toHaveCount(0);
    await expect(dialog.getByText("没有匹配的商品，请调整名称或编码")).toBeVisible();
    await search.fill("SKU-2");
    await expect(dialog.getByRole("listbox", { name: "第 1 行商品", exact: true }).getByRole("option")).toHaveCount(1);
    await search.press("Enter");
    await expect(product).toContainText("sku-2");
    await expect(search).toHaveCount(0);
    const warehouseLabel = purchase ? "收货仓库" : "仓库";
    await dialog.getByLabel(`第 1 行${warehouseLabel}`, { exact: true }).click();
    const warehouseSearch = dialog.getByRole("combobox", { name: `搜索第 1 行${warehouseLabel}`, exact: true });
    await warehouseSearch.fill("warehouse-2");
    await warehouseSearch.press("Enter");
    await dialog
      .getByRole("button", { name: purchase ? "+ 添加采购行" : "+ 添加商品行" })
      .click();
    await dialog
      .getByLabel("法律主体", { exact: true })
      .selectOption("legal_entity-2");
    await expect(product).toContainText("sku-2");
    await expect(dialog.getByRole("combobox", { name: "第 2 行商品", exact: true })).toContainText("sku-1");
    for (const width of [520, 375]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(dialog).toBeVisible();
      expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      expect(await dialog.locator(".entry-line").first().evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      await product.click();
      await expect(dialog.getByRole("combobox", { name: "搜索第 1 行商品", exact: true })).toBeVisible();
      expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      if (width === 520) {
        await waitForAnimations(page);
        await dialog.screenshot({ path: `test-results/${flow}-entry-narrow-search.png` });
      }
      await page.keyboard.press("Escape");
      await expect(dialog).toBeVisible();
    }
    await expect(party).toContainText(`${purchase ? "supplier" : "customer"}-2`);
    await expect(
      dialog.getByLabel(`第 1 行${warehouseLabel}`, { exact: true }),
    ).toContainText("warehouse-2");
    await expect(
      dialog.getByLabel(`第 2 行${warehouseLabel}`, { exact: true }),
    ).toContainText("warehouse-1");
    const quantity = dialog.getByRole("spinbutton", { name: "第 1 行数量", exact: true });
    await quantity.fill("0");
    await dialog.getByRole("button", { name: purchase ? "保存采购订单草稿" : "保存销售订单草稿", exact: true }).click();
    await expect(quantity).toBeFocused();
    await expect(quantity).toHaveAttribute("aria-invalid", "true");
    await expect(dialog.getByRole("alert")).toContainText("第 1 行数量");
    await dialog.getByRole("button", { name: "请检查第 1 行数量的格式或取值范围", exact: true }).click();
    await expect(quantity).toBeFocused();
    await quantity.fill("1");
    await expect(quantity).not.toHaveAttribute("aria-invalid", "true");
    await dialog.getByRole("button", { name: "关闭弹窗", exact: true }).click();
    const prompt = page.getByRole("dialog", { name: "放弃未保存修改", exact: true });
    await expect(prompt).toBeVisible();
    await prompt.getByRole("button", { name: "继续编辑", exact: true }).click();
    await expect(quantity).toHaveValue("1");
    await page.setViewportSize({ width: 1366, height: 900 });
    await page.mouse.click(1, 1);
    await expect(prompt).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(prompt).toHaveCount(0);
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(prompt).toBeVisible();
    await prompt.getByRole("button", { name: "放弃修改", exact: true }).click();
    await expect(dialog).toHaveCount(0);


  });
}
