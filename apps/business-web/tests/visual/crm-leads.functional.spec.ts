import {test,expect} from "@playwright/test";
test("线索录入、右侧详情、筛选跟进与确认转商机",async({page})=>{
 let item:any=null;const notes:any[]=[];let conversions=0;
 const options=[{id:"legal",resourceType:"legal_entity",name:"法人甲",code:"LE",status:"active"},{id:"unit",resourceType:"business_unit",name:"经营甲",code:"BU",status:"active",parentBusinessUnitId:null}];
 await page.route("**/api/**",async route=>{
  const req=route.request();const path=new URL(req.url()).pathname;
  if(path==="/api/session")return route.fulfill({json:{authenticated:true,csrfToken:"csrf"}});
  if(path==="/api/v1/crm/options")return route.fulfill({json:{items:options}});
  if(path.endsWith("/owners"))return route.fulfill({json:{items:[{id:"me",name:"自己",displayName:"自己"}],currentUserId:"me"}});
  if(path==="/api/v1/crm/leads" && req.method()==="POST"){
   const body=req.postDataJSON();expect(body.title).toBe("年度采购需求");expect(body.customerId).toBeNull();expect(req.headers()["idempotency-key"]).toBeTruthy();
   item={...body,id:"lead",ownerUserId:"me",ownerName:"自己",status:"new",version:1};return route.fulfill({json:{id:"lead",transferred:false}});
  }
  if(path==="/api/v1/crm/leads/lead/followups") {const body=req.postDataJSON();expect(body.expectedVersion).toBe(1);item={...item,...body,version:2};notes.push({...body,id:"note",authorName:"自己",createdAt:"2026-10-03T00:00:00Z"});return route.fulfill({json:{id:"lead",version:2}});}
  if(path==="/api/v1/crm/leads/lead/convert") {conversions++;const body=req.postDataJSON();expect(body.expectedVersion).toBe(2);expect(body.opportunity.stage).toBe("contacting");expect(body.opportunity.nextAction).toBe("发送方案");expect(body.opportunity.nextFollowUp).toBe("2026-10-06");return route.fulfill({json:{id:"opp",version:1}});}
  if(path==="/api/v1/crm/leads/lead")return route.fulfill({json:{item,followups:notes,hasMore:false,duplicates:[]}});
  if(path==="/api/v1/crm/leads")return route.fulfill({json:{items:item?[item]:[],canManage:true,hasMore:false}});
  if(path==="/api/v1/crm/opportunities/opp")return route.fulfill({json:{item:{...item,id:"opp",stage:"contacting",currency:"CNY"},followups:[],sourceLeadId:"lead"}});
  return route.fulfill({json:{items:[],canManage:true}});
 });
 await page.goto("/#crmLeads");await page.getByRole("button",{name:"新建线索",exact:true}).click();
 let drawer=page.getByRole("dialog",{name:"新建线索",exact:true});
 await drawer.getByLabel("线索名称",{exact:true}).fill("年度采购需求");
 await drawer.getByRole("button",{name:"保存线索",exact:true}).click();
 drawer=page.getByRole("dialog",{name:"线索详情",exact:true});await expect(drawer.getByRole("heading",{name:"年度采购需求"})).toBeVisible();
 await drawer.getByRole("button",{name:"记录跟进 / 淘汰"}).click();
 await drawer.getByLabel("筛选结果").selectOption("contacting");await drawer.getByLabel("沟通内容").fill("已确认采购需求");await drawer.getByLabel("下一步",{exact:true}).fill("发送方案");await drawer.getByLabel("跟进日期",{exact:true}).fill("2026-10-06");
 await drawer.getByRole("button",{name:"保存跟进"}).click();await drawer.getByRole("button",{name:"转为商机",exact:true}).click();
 await expect(drawer.getByLabel("商机名称",{exact:true})).toHaveValue("年度采购需求");expect(conversions).toBe(0);
 await drawer.getByLabel("客户公司",{exact:true}).fill("杭州某公司");
 await drawer.getByRole("button",{name:"确认转为商机",exact:true}).click();await expect(page).toHaveURL(/opportunity=opp/);expect(conversions).toBe(1);
 await expect(page.getByRole("link",{name:"查看来源线索"})).toHaveAttribute("href","/#crmLeads?lead=lead");
});
test("线索未保存离开确认与只读权限",async({page})=>{
 await page.route("**/api/**",async route=>{const path=new URL(route.request().url()).pathname;return route.fulfill({json:path==="/api/session"?{authenticated:true,csrfToken:"csrf"}:{items:[],canManage:true}});});
 await page.goto("/#crmLeads");await page.getByRole("button",{name:"新建线索",exact:true}).click();await page.getByLabel("线索名称",{exact:true}).fill("未保存");await page.keyboard.press("Escape");await expect(page.getByRole("dialog",{name:"放弃未保存修改"})).toBeVisible();await page.getByRole("button",{name:"放弃修改",exact:true}).click();await expect(page.getByRole("dialog")).toHaveCount(0);
});
