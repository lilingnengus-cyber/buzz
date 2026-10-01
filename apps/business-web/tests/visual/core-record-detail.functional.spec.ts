import { expect, test } from "@playwright/test";

for (const canManage of [true, false]) {
  test(`核心记录点击详情、权限与外部关闭 canManage=${canManage}`, async ({
    page,
  }) => {
    const writes: unknown[] = [];
    const record = {
      id: "legal",
      resourceType: "legal_entity",
      code: "LE-001",
      name: "示例法人",
      countryCode: "CN",
      functionalCurrency: "CNY",
      status: "active",
      version: 4,
      updatedAt: "2026-10-01T00:00:00Z",
    };
    await page.route("**/api/**", async (route) => {
      const req = route.request(),
        path = new URL(req.url()).pathname;
      if (path === "/api/session")
        await route.fulfill({
          json: {
            authenticated: true,
            subject: "test",
            displayName: "测试",
            csrfToken: "csrf",
          },
        });
      else if (req.method() === "PUT") {
        writes.push(req.postDataJSON());
        await route.fulfill({ json: { id: "legal", version: 5 } });
      } else if (path === "/api/v1/core-master-data")
        await route.fulfill({
          json: {
            items: [record],
            canManage,
            dataAsOf: "2026-10-01T00:00:00Z",
          },
        });
      else await route.fulfill({ json: { items: [] } });
    });
    await page.goto("/#coreData");
    const row = page.getByRole("article", { name: "查看示例法人详情" });
    await row.getByText("示例法人", { exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "法定主体详情" });
    const name = dialog.getByRole("textbox", { name: "名称 *" });
    await expect(name).toHaveValue("示例法人");
    await expect(
      dialog.getByRole("textbox", { name: "编码", exact: true }),
    ).toBeDisabled();
    if (canManage) {
      await name.fill("修改法人");
      await expect(dialog).toBeVisible();
      await dialog.getByRole("button", { name: "保存修订" }).click();
      await expect(dialog).toHaveCount(0);
      expect(writes[0]).toMatchObject({
        name: "修改法人",
        expectedVersion: 4,
        code: "LE-001",
      });
    } else {
      await expect(name).toBeDisabled();
      await expect(
        dialog.getByRole("button", { name: "保存修订" }),
      ).toHaveCount(0);
      await dialog.getByRole("heading").click();
      await expect(dialog).toBeVisible();
      await page.mouse.click(10, 10);
      await expect(dialog).toHaveCount(0);
    }
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(dialog).toBeVisible();
    const before = writes.length;
    await page.mouse.click(10, 10);
    await expect(dialog).toHaveCount(0);
    expect(writes).toHaveLength(before);
    if (canManage) {
      await row.getByRole("button", { name: "停用", exact: true }).click();
      await expect(
        page.getByRole("dialog", { name: "停用法定主体" }),
      ).toBeVisible();
      await expect(dialog).toHaveCount(0);
    }
  });
}
