import { expect, test } from "@playwright/test";

test("编辑经营主体后刷新负责人候选，失败可重试并保存", async ({ page }) => {
  let item = {
    id: "op-1", legalEntityId: "le", businessUnitId: "bu",
    customerId: null, title: "年度采购", companyName: "客户甲",
    contactName: "", contactDetails: "", stage: "contacting",
    expectedAmountMinor: null, currency: "CNY", nextAction: "",
    nextFollowUp: null, ownerUserId: "user-1", ownerName: "原负责人",
    expectedCloseDate: null, lossReason: "", version: 1,
  };
  let targetReads = 0;
  let writes = 0;
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const path = url.pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf" } });
    if (path === "/api/v1/crm/options") return route.fulfill({ json: { items: [
      { id: "le", name: "法人", code: "LE", resourceType: "legal_entity", status: "active" },
      ...["bu", "target"].map((id) => ({ id, name: id === "bu" ? "原经营单元" : "目标经营单元", code: id.toUpperCase(), resourceType: "business_unit", status: "active", ancestorPath: [] })),
    ] } });
    if (path === "/api/v1/crm/owners") {
      expect(url.searchParams.get("legalEntityId")).toBe("le");
      if (url.searchParams.get("businessUnitId") === "target") {
        targetReads++;
        if (targetReads === 1) return route.fulfill({ status: 503, json: { error: "暂不可用" } });
        return route.fulfill({ json: { items: [{ id: "user-2", name: "目标负责人" }], hasMore: false } });
      }
      return route.fulfill({ json: { items: [{ id: "user-1", name: "原负责人" }], hasMore: false } });
    }
    if (path === "/api/v1/crm/opportunities/op-1" && req.method() === "PUT") {
      const input = req.postDataJSON();
      expect(input.businessUnitId).toBe("target");
      expect(input.ownerUserId).toBe("user-2");
      expect(input.expectedVersion).toBe(1);
      writes++;
      item = { ...item, ...input, ownerName: "目标负责人", version: 2 };
      return route.fulfill({ json: { id: item.id, version: 2 } });
    }
    if (path === "/api/v1/crm/opportunities/op-1") return route.fulfill({ json: { item, followups: [], hasOlderFollowups: false } });
    if (path === "/api/v1/crm/opportunities") return route.fulfill({ json: { items: [item], hasMore: false, canManage: true } });
    return route.fulfill({ json: { items: [], hasMore: false } });
  });
  await page.goto("/#crm");
  await page.locator(".crm-row").click();
  const drawer = page.getByRole("dialog", { name: "商机详情", exact: true });
  await drawer.getByRole("button", { name: "编辑商机" }).click();
  await drawer.getByRole("button", { name: /当前选择.*原经营单元/ }).click();
  await drawer.getByRole("tree", { name: "经营主体", exact: true }).getByRole("button", { name: "TARGET 目标经营单元", exact: true }).click();
  await expect(drawer.getByRole("alert")).toContainText("负责人列表加载失败");
  await drawer.getByRole("button", { name: "重新读取负责人" }).click();
  await expect(drawer.getByRole("alert")).toHaveCount(0);
  await drawer.getByRole("combobox", { name: "商机负责人", exact: true }).click();
  await expect(drawer.getByRole("option", { name: /原负责人/ })).toHaveCount(0);
  await drawer.getByRole("option", { name: "目标负责人", exact: true }).click();
  await drawer.getByRole("button", { name: "保存商机", exact: true }).click();
  await expect(drawer.getByText("目标负责人", { exact: true })).toBeVisible();
  await expect.poll(() => writes).toBe(1);
  expect(targetReads).toBe(2);
});
