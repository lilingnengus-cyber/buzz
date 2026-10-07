import { expect, test } from "@playwright/test";

for (const kind of [
  "legal_entity",
  "business_unit",
  "customer",
  "supplier",
  "warehouse",
]) {
  test(`${kind}: delete confirmation, failure, retry and refresh`, async ({
    page,
  }) => {
    const labels: Record<string, string> = {
      legal_entity: "法定主体",
      business_unit: "经营主体",
      customer: "客户",
      supplier: "供应商",
      warehouse: "仓库",
    };
    let deleted = false;
    let attempts = 0;
    let firstKey = "";
    await page.route("**/api/session", (route) =>
      route.fulfill({ json: { csrfToken: "test-csrf" } }),
    );
    await page.route("**/api/v1/core-master-data**", async (route) => {
      const req = route.request();
      if (req.method() === "DELETE") {
        attempts++;
        expect(req.url()).toContain(`/${kind}/record-1`);
        expect(req.postDataJSON()).toEqual({ expectedVersion: 3 });
        expect(req.headers()["x-csrf-token"]).toBe("test-csrf");
        const key = req.headers()["idempotency-key"];
        expect(key).toBeTruthy();
        if (attempts === 1) {
          firstKey = key;
          await route.fulfill({
            status: 400,
            json: { message: "该记录已被引用，请改用停用。" },
          });
        } else {
          expect(key).toBe(firstKey);
          deleted = true;
          await route.fulfill({ json: { status: "deleted" } });
        }
        return;
      }
      await route.fulfill({
        json: {
          canManage: true,
          dataAsOf: "2026-10-07",
          items: deleted
            ? []
            : [
                {
                  id: "record-1",
                  resourceType: kind,
                  code: "TEST-01",
                  name: "测试记录",
                  status: "active",
                  version: 3,
                  updatedAt: "2026-10-07",
                },
              ],
        },
      });
    });
    await page.goto("/#coreData");
    await page.getByRole("tab").filter({ hasText: labels[kind] }).click();
    await page.getByRole("button", { name: "删除", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("TEST-01");
    await dialog.getByRole("button", { name: "取消", exact: true }).click();
    expect(attempts).toBe(0);
    await page.getByRole("button", { name: "删除", exact: true }).click();
    await dialog.getByRole("button", { name: "确认删除" }).click();
    await expect(dialog.getByRole("alert")).toContainText("已被引用");
    await dialog.getByRole("button", { name: "确认删除" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByText("测试记录", { exact: true })).toBeHidden();
    await expect(
      page.getByRole("heading", { name: `尚无符合条件的${labels[kind]}` }),
    ).toBeVisible();
  });
}

test("read-only users have no delete action", async ({ page }) => {
  await page.route("**/api/v1/core-master-data**", (route) =>
    route.fulfill({
      json: {
        canManage: false,
        dataAsOf: "2026-10-07",
        items: [
          {
            id: "record-1",
            resourceType: "legal_entity",
            code: "TEST-01",
            name: "测试记录",
            status: "active",
            version: 1,
            updatedAt: "2026-10-07",
          },
        ],
      },
    }),
  );
  await page.goto("/#coreData");
  await expect(page.getByText("只读权限")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "删除", exact: true }),
  ).toHaveCount(0);
});
