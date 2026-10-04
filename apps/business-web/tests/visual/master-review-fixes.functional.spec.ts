import { test, expect } from "@playwright/test";

for (const product of [false, true]) {
  test(`${product ? "商品" : "核心"}资料分类独立筛选，启停提交禁止关闭`, async ({
    page,
  }) => {
    const endpoint = `/api/v1/${product ? "product" : "core"}-master-data`;
    const types = product ? ["product", "brand"] : ["legal_entity", "customer"];
    const items = types.map((resourceType, i) => ({
      resourceType,
      id: String(i),
      code: `TEST-${i}`,
      name: `资料${i}`,
      status: i ? "disabled" : "active",
      version: 1,
      countryCode: "CN",
      functionalCurrency: "CNY",
      unitOfMeasureCode: "EA",
      updatedAt: "2026-10-05T00:00:00Z",
    }));
    let release: (() => void) | undefined;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route("**/api/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/session")
        return route.fulfill({
          json: { authenticated: true, csrfToken: "csrf" },
        });
      if (path === endpoint)
        return route.fulfill({
          json: { items, canManage: true, dataAsOf: "2026-10-05T00:00:00Z" },
        });
      if (path.endsWith("/disable-impact"))
        return route.fulfill({ json: { canDisable: true, impacts: [] } });
      if (path.endsWith("/status")) {
        await pending;
        return route.fulfill({ status: 409, json: { message: "版本冲突" } });
      }
      return route.fulfill({ json: { items: [] } });
    });
    await page.goto(product ? "/#productData" : "/#coreData");
    const tabs = page.getByRole("tab");
    const other = product
      ? page.getByRole("tab", { name: /^BRD 品牌 / })
      : tabs.filter({ hasText: "客户" });
    const first = product
      ? page.getByRole("tab", { name: /^SPU 商品 / })
      : tabs.filter({ hasText: "法定主体" });
    const search = page.getByRole("textbox", { name: "检索", exact: true });
    const status = page.getByRole("combobox", { name: "状态", exact: true });
    await search.fill("资料0");
    await status.selectOption("active");
    await other.click();
    await expect(search).toHaveValue("");
    await expect(status).toHaveValue("all");
    await search.fill("资料1");
    await status.selectOption("disabled");
    await first.click();
    await expect(search).toHaveValue("资料0");
    await expect(status).toHaveValue("active");
    await other.click();
    await expect(search).toHaveValue("资料1");
    await expect(status).toHaveValue("disabled");
    await first.click();
    await page.getByRole("button", { name: "停用", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: "确认停用", exact: true }).click();
    await expect(
      dialog.getByRole("button", { name: "取消", exact: true }),
    ).toBeDisabled();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeVisible();
    await expect(
      page.getByRole("button", { name: "关闭弹窗", exact: true }).first(),
    ).toBeDisabled();
    release!();
    await expect(
      dialog.getByRole("button", { name: "取消", exact: true }),
    ).toBeEnabled();
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
    await expect(dialog).not.toBeVisible();
  });
}

test("编码规则脏数据退出确认、保存失败保留，提交期间禁止关闭", async ({
  page,
}) => {
  let release: (() => void) | undefined;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  const rule = {
    recordType: "customer",
    name: "客户编码",
    status: "active",
    resetPeriod: "never",
    scopeDimension: "global",
    segments: [
      { type: "literal", value: "CU-" },
      { type: "sequence", width: 6 },
    ],
    version: 1,
    updatedAt: "2026-10-05T00:00:00Z",
  };
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf" },
      });
    if (route.request().method() === "PUT") {
      await pending;
      return route.fulfill({
        status: 409,
        json: { message: "版本冲突，请重试" },
      });
    }
    return route.fulfill({ json: { items: [rule], canManage: true } });
  });
  await page.goto("/#numbering");
  await page.getByRole("button", { name: "编辑规则", exact: true }).click();
  const editor = page.getByRole("dialog", {
    name: "编辑客户编码",
    exact: true,
  });
  const name = editor.getByRole("textbox", { name: "规则名称", exact: true });
  await name.fill("修改后客户编码");
  await page.keyboard.press("Escape");
  const prompt = page.getByRole("dialog", {
    name: "放弃未保存修改",
    exact: true,
  });
  await expect(prompt).toBeVisible();
  await prompt.getByRole("button", { name: "继续编辑" }).click();
  await expect(name).toHaveValue("修改后客户编码");
  await editor.getByRole("button", { name: "保存规则", exact: true }).click();
  await expect(
    editor.getByRole("button", { name: "取消", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "关闭编码规则编辑器", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(editor).toBeVisible();
  release!();
  await expect(editor.getByText("版本冲突，请重试")).toBeVisible();
  await expect(name).toHaveValue("修改后客户编码");
  await editor.getByRole("button", { name: "关闭", exact: true }).click();
  await prompt.getByRole("button", { name: "放弃修改" }).click();
  await expect(editor).not.toBeVisible();
});
