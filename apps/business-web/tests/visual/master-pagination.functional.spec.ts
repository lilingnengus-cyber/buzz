import { test, expect } from "@playwright/test";

for (const product of [false, true]) {
  test(`${product ? '商品' : '核心'}资料跨页、服务端搜索与筛选重置`, async ({ page }) => {
    const type = product ? 'product' : 'legal_entity';
    const endpoint = `/api/v1/${product ? 'product' : 'core'}-master-data`;
    const queries: URLSearchParams[] = [];
    await page.route('**/api/**', async route => {
      const url = new URL(route.request().url());
      if (url.pathname === '/api/session') return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
      if (url.pathname !== endpoint) return route.fulfill({json:{items:[]}});
      const p=url.searchParams; queries.push(p);
      const offset=Number(p.get('offset')); const filtered=!!p.get('query') || p.get('status') === 'disabled';
      const n=filtered ? 1100 : offset+1;
      return route.fulfill({json:{items:[{id:String(n),resourceType:type,code:`TEST-${n}`,name:`资料${n}`,status:p.get('status')==='disabled'?'disabled':'active',version:1,countryCode:'CN',functionalCurrency:'CNY',unitOfMeasureCode:'EA',updatedAt:'2026-10-05T00:00:00Z'}],total:filtered?1:1100,hasMore:!filtered && offset<1050,counts:{[type]:1100},canManage:true,dataAsOf:'2026-10-05T00:00:00Z'}});
    });
    await page.goto(product ? '/#productData' : '/#coreData');
    const paging=page.getByRole('navigation',{name:'资料分页'});
    await expect(paging).toContainText('共 1100 条');
    await paging.getByRole('button',{name:'下一页'}).click();
    await expect(page.getByText('TEST-51',{exact:true})).toBeVisible();
    await expect(paging).toContainText('第 2 页');
    await page.getByRole('textbox',{name:'检索',exact:true}).fill('后端关联字段');
    await expect(page.getByText('TEST-1100',{exact:true})).toBeVisible();
    await expect(paging).toContainText('共 1 条，第 1 页');
    await expect(paging.getByRole('button',{name:'下一页'})).toBeDisabled();
    expect(queries.at(-1)?.get('offset')).toBe('0');
    expect(queries.at(-1)?.get('query')).toBe('后端关联字段');
    await page.getByRole('combobox',{name:'状态',exact:true}).selectOption('disabled');
    await expect.poll(()=>queries.at(-1)?.get('status')).toBe('disabled');
    await expect(page.getByText('TEST-1100',{exact:true})).toBeVisible();
  });
}

test('经营组织加载后续页保持父子树完整',async({page})=>{
  const offsets:string[]=[];
  await page.route('**/api/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/api/session') return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    if(url.pathname!=='/api/v1/core-master-data') return route.fulfill({json:{items:[]}});
    if(url.searchParams.get('resourceType')!=='business_unit') return route.fulfill({json:{items:[],total:0,hasMore:false,dataAsOf:'2026-10-05T00:00:00Z',canManage:true,counts:{business_unit:2}}});
    const offset=url.searchParams.get('offset')??'0';offsets.push(offset);
    const root={id:'root',resourceType:'business_unit',code:'ROOT',name:'总部',isOperatingRoot:true,status:'active',version:1,updatedAt:'2026-10-05T00:00:00Z',ancestorPath:['总部']};
    const child={...root,id:'child',code:'CHILD',name:'分页子部门',isOperatingRoot:false,parentBusinessUnitId:'root',ancestorPath:['总部','分页子部门']};
    return route.fulfill({json:{items:[offset==='0'?root:child],total:2,hasMore:offset==='0',dataAsOf:'2026-10-05T00:00:00Z',canManage:true,counts:{business_unit:2}}});
  });
  await page.goto('/#coreData');
  await page.getByRole('tab').filter({hasText:'经营主体'}).click();
  await expect(page.getByText('分页子部门',{exact:true}).first()).toBeVisible();
  expect(offsets).toEqual(['0','1']);
});

test('编辑商品候选品牌包含第二页', async ({page})=>{
  const seen:string[]=[];
  await page.route('**/api/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/api/session') return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    if(url.pathname!=='/api/v1/product-master-data') return route.fulfill({json:{items:[]}});
    const type=url.searchParams.get('resourceType'),offset=url.searchParams.get('offset');
    const id=type==='brand' && offset!=='0'?'second':'first';
    if(type==='brand') seen.push(offset??'');
    return route.fulfill({json:{items:[{id,resourceType:type,code:id,name:id==='second'?'末页品牌':'首项',status:'active',version:1,unitOfMeasureCode:'EA',updatedAt:'2026-10-05T00:00:00Z'}],total:type==='brand'?2:1,hasMore:type==='brand' && offset==='0',canManage:true,dataAsOf:'2026-10-05T00:00:00Z'}});
  });
  await page.goto('/#productData');
  await page.getByRole('button',{name:'＋ 新增商品',exact:true}).click();
  const brand=page.getByRole('dialog').getByRole('combobox',{name:'品牌',exact:true});
  await expect(brand.locator('option')).toContainText(['无品牌','首项','末页品牌']);
  await brand.selectOption('second');
  await expect(brand).toHaveValue('second');
  expect(seen).toEqual(['0','1']);
});

test('客户链接按 id 定位到首屏之外的客户',async({page})=>{
  let exactLookup=false;
  const record={id:'target',resourceType:'customer',code:'CU-9999',name:'末页客户',status:'active',version:1,updatedAt:'2026-10-05T00:00:00Z'};
  await page.route('**/api/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/api/session')return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    if(url.pathname!=='/api/v1/core-master-data')return route.fulfill({json:{items:[]}});
    const match=url.searchParams.get('id')==='target'||url.searchParams.get('query')==='CU-9999';
    if(url.searchParams.get('id')==='target')exactLookup=true;
    return route.fulfill({json:{items:match?[record]:[],total:match?1:0,hasMore:false,canManage:true,dataAsOf:'2026-10-05T00:00:00Z'}});
  });
  await page.goto('/customers/target');
  await expect(page.getByText('末页客户',{exact:true})).toBeVisible();
  expect(exactLookup).toBe(true);
});
