import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("分配负责人、预计成交日期、流失原因与重新跟进", async ({ page }) => {
  let item: any = {
    id: "op-1",
    legalEntityId: "le",
    businessUnitId: "bu",
    customerId: null,
    title: "年度采购",
    companyName: "客户甲",
    contactName: "张经理",
    contactDetails: "",
    stage: "contacting",
    expectedAmountMinor: null,
    currency: "CNY",
    nextAction: "提交方案",
    nextFollowUp: null,
    ownerUserId: "user-1",
    ownerName: "当前同事",
    expectedCloseDate: null,
    lossReason: "",
    version: 1,
  };
  const notes: any[] = [];
  let writes = 0;
  await page.route("**/api/**", async (route) => {
    const req = route.request(),
      url = new URL(req.url()),
      path = url.pathname;
    if (path === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf" },
      });
    if (path === "/api/v1/crm/options")
      return route.fulfill({
        json: {
          items: [
            {
              id: "le",
              name: "法人",
              code: "LE",
              resourceType: "legal_entity",
              status: "active",
            },
            {
              id: "bu",
              name: "经营单元",
              code: "OU",
              resourceType: "business_unit",
              status: "active",
              ancestorPath: ["经营单元"],
            },
          ],
        },
      });
    if (path === "/api/v1/crm/owners")
      return route.fulfill({
        json: {
          items: [
            { id: "user-1", name: "当前同事" },
            { id: "user-2", name: "李同事" },
          ],
          hasMore: false,
          currentUserId: "user-1",
        },
      });
    if (path === "/api/v1/crm/accounts" || path === "/api/v1/crm/contacts")
      return route.fulfill({ json: { items: [], hasMore: false } });
    if (path === "/api/v1/crm/opportunities/op-1" && req.method() === "PUT") {
      const input = req.postDataJSON();
      expect(input.expectedVersion).toBe(item.version);
      writes++;
      item = {
        ...item,
        ...input,
        ownerName: input.ownerUserId === "user-2" ? "李同事" : "当前同事",
        version: item.version + 1,
      };
      return route.fulfill({ json: { id: item.id, version: item.version } });
    }
    if (path.endsWith("/followups") && req.method() === "POST") {
      const input = req.postDataJSON();
      expect(input.expectedVersion).toBe(item.version);
      writes++;
      notes.unshift({
        ...input,
        id: `note-${writes}`,
        authorName: "当前同事",
        createdAt: "2026-10-02T01:00:00Z",
      });
      item = { ...item, ...input, version: item.version + 1 };
      return route.fulfill({ json: { id: item.id, version: item.version } });
    }
    if (path === "/api/v1/crm/opportunities/op-1")
      return route.fulfill({
        json: { item, followups: notes, hasOlderFollowups: false },
      });
    if (path === "/api/v1/crm/opportunities")
      return route.fulfill({
        json: {
          items:
            url.searchParams.get("mine") === "true" &&
            item.ownerUserId !== "user-1"
              ? []
              : [item],
          hasMore: false,
          canManage: true,
        },
      });
    return route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#crm");
  await page.locator(".crm-row").click();
  const drawer = page.getByRole("dialog", { name: "商机详情", exact: true });
  await drawer.getByRole("button", { name: "编辑商机" }).click();
  await drawer
    .getByRole("combobox", { name: "商机负责人", exact: true })
    .selectOption("user-2");
  await drawer.getByLabel("预计成交日期").fill("2026-11-30");
  await drawer.getByRole("button", { name: "保存商机", exact: true }).click();
  await expect(drawer.getByText("李同事", { exact: true })).toBeVisible();
  await expect(drawer.getByText("2026-11-30", { exact: true })).toBeVisible();
  await drawer.getByLabel("本次沟通").fill("客户预算暂停");
  await drawer.getByLabel("更新阶段").selectOption("lost");
  await drawer.getByRole("button", { name: "保存跟进", exact: true }).click();
  expect(writes).toBe(1);
  await expect(drawer.getByLabel("流失原因", { exact: true })).toBeFocused();
  await drawer.getByLabel("流失原因", { exact: true }).fill("预算取消");
  await drawer.getByRole("button", { name: "保存跟进", exact: true }).click();
  await expect(
    drawer.locator(".crm-next").filter({ hasText: "流失原因" }),
  ).toContainText("预算取消");
  await drawer.getByLabel("本次沟通").fill("客户重新启动项目");
  await drawer.getByLabel("更新阶段").selectOption("contacting");
  await expect(drawer.getByLabel("流失原因", { exact: true })).toHaveCount(0);
  await drawer.getByRole("button", { name: "保存跟进", exact: true }).click();
  await expect(drawer.locator(".crm-history")).toContainText(
    "流失原因：预算取消",
  );
  await expect(drawer.locator(".crm-history")).toContainText(
    "客户重新启动项目",
  );
  expect(item.lossReason).toBe("");
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/crm-owner-outcome.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "我的商机", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "没有符合条件的商机" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "我的商机", exact: true }).click();
  await expect(page.locator(".crm-row")).toContainText("李同事");
});
