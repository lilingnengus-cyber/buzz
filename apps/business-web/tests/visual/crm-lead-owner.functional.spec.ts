import { expect, test } from "@playwright/test";

test("线索负责人鼠标选择、失败重试及重新打开保留", async ({ page }) => {
  let item = { id: "lead", title: "负责人验收", companyName: "", contactName: "", contactDetails: "", source: "", summary: "", nextAction: "历史安排", nextFollowUp: "2026-10-06", ownerUserId: "old", ownerName: "原负责人", customerId: null, status: "new", version: 1 };
  let attempts = 0;
  const keys: string[] = [];
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    if (path === "/api/session") return route.fulfill({ json: { authenticated: true, csrfToken: "csrf" } });
    if (path.endsWith("/owners")) return route.fulfill({ json: { items: [{id:"old",name:"原负责人"},{id:"new",name:"新负责人"}], currentUserId:"old" } });
    if (path === "/api/v1/crm/leads/lead" && req.method() === "PUT") {
      const body = req.postDataJSON();
      expect(body.ownerUserId).toBe("new");
      expect(body.nextAction).toBe("历史安排");
      expect(body.nextFollowUp).toBe("2026-10-06");
      keys.push(req.headers()["idempotency-key"]);
      attempts++;
      if (attempts === 1) return route.fulfill({status:503,json:{error:"暂时无法保存"}});
      item = {...item,...body,ownerName:"新负责人",version:2};
      return route.fulfill({json:{id:"lead",version:2,transferred:true}});
    }
    if (path === "/api/v1/crm/leads/lead") return route.fulfill({json:{item,followups:[],duplicates:[],hasMore:false}});
    return route.fulfill({json:{items:[item],canManage:true,hasMore:false}});
  });
  await page.goto("/#crmLeads");
  await expect(page.getByRole("columnheader")).toHaveText([
    "线索名称", "公司名称", "联系人", "联系方式", "线索来源", "需求摘要",
    "负责人", "状态", "淘汰原因", "已转商机", "创建时间", "更新时间",
  ]);
  await page.setViewportSize({width:520,height:780});
  const tableRegion = page.getByRole("region", {name:"线索完整资料"});
  await expect(tableRegion).toBeVisible();
  expect(await tableRegion.evaluate(el => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(520);
  await page.setViewportSize({width:1366,height:768});
  await page.locator(".crm-lead-title").click();
  const drawer=page.getByRole("dialog",{name:"线索详情",exact:true});
  await drawer.getByRole("combobox",{name:"负责人",exact:true}).click();
  const option=drawer.getByRole("option",{name:"新负责人",exact:true});
  // Emulate native WebKit moving focus outside the picker on mouse-down.
  // A cancelled mouse-down must retain search focus until the click arrives.
  await page.evaluate(() => {
    document.addEventListener("mousedown", event => {
      if ((event.target as HTMLElement).closest('[role="option"]') && !event.defaultPrevented) {
        (document.querySelector('input[name="title"]') as HTMLInputElement).focus();
      }
    });
  });
  await option.click();
  await expect(drawer.getByRole("combobox",{name:"负责人",exact:true})).toContainText("新负责人");
  await drawer.getByRole("button",{name:"保存线索",exact:true}).click();
  await expect(drawer.getByRole("alert")).toBeVisible();
  await expect(drawer.getByRole("combobox",{name:"负责人",exact:true})).toContainText("新负责人");
  await drawer.getByRole("button",{name:"保存线索",exact:true}).click();
  await expect(drawer).toHaveCount(0);
  await page.locator(".crm-lead-row").click();
  await expect(drawer.getByRole("combobox",{name:"负责人",exact:true})).toContainText("新负责人");
  expect(attempts).toBe(2);
  expect(keys[0]).toBe(keys[1]);
});
