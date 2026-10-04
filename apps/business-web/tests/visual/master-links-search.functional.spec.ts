import { expect, test } from "@playwright/test";

const date = "2026-10-05T00:00:00Z";
const kinds = [
  ["customer", "customers", "客户"], ["supplier", "suppliers", "供应商"],
  ["warehouse", "warehouses", "仓库"], ["product", "products", "商品"], ["sku", "skus", "SKU / 条码"],
];
for (const [type, path, label] of kinds) for (const canManage of [true, false]) {
  test(`${label}直达详情 ${canManage ? '可编辑保存' : '只读'}，支持嵌入路由`, async ({page}) => {
    let saved: Record<string, unknown> | undefined;
    const record = {id:'target',resourceType:type,code:'REC-1',name:'直达记录',status:'active',version:4,creditCurrency:'CNY',creditLimitMinor:0,paymentTermsDays:30,updatedAt:date,unitOfMeasureCode:'EA'};
    await page.route('**/api/**',async route=>{
      const url=new URL(route.request().url());
      if(url.pathname==='/api/session') return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
      if(route.request().method()==='PUT'){
        expect(url.pathname).toContain(`/${type}/target`);
        saved=route.request().postDataJSON();
        return route.fulfill({json:{id:'target',version:5}});
      }
      if(url.pathname.endsWith('master-data')) return route.fulfill({json:{items:url.searchParams.get('resourceType')===type?[record]:[],canManage,total:1,hasMore:false,dataAsOf:date}});
      return route.fulfill({json:{items:[]}});
    });
    await page.goto(`${canManage?'/embed':''}/${path}/target`);
    const dialog=page.getByRole('dialog',{name:`${label}详情`,exact:true});
    const name=dialog.getByRole('textbox',{name:'名称 *',exact:true});
    await expect(name).toHaveValue('直达记录');
    await expect(dialog.getByRole('link',{name:'详情链接'})).toHaveAttribute('href',`${canManage?'/embed':''}/${path}/target`);
    if(canManage){
      await name.fill('修改直达记录');
      await page.keyboard.press('Escape');
      await expect(page.getByRole('dialog',{name:'放弃未保存修改'})).toBeVisible();
      await page.getByRole('button',{name:'继续编辑',exact:true}).click();
      await dialog.getByRole('button',{name:'保存修订',exact:true}).click();
      await expect(dialog).toHaveCount(0);
      expect(saved).toMatchObject({name:'修改直达记录',expectedVersion:4});
    } else {
      await expect(name).toBeDisabled();
      await expect(dialog.getByRole('button',{name:'保存修订'})).toHaveCount(0);
      await page.mouse.click(5,5);
      await expect(dialog).toHaveCount(0);
      expect(saved).toBeUndefined();
    }
  });
}

test('失效或无权限链接不会误开别的记录，支持重试',async({page})=>{
  let allow=false;
  await page.route('**/api/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/api/session')return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    if(url.pathname.endsWith('master-data'))return route.fulfill({json:{items:allow?[{id:'target',resourceType:'supplier',code:'SU-1',name:'恢复记录',status:'active',version:1,updatedAt:date}]:[],canManage:true,total:allow?1:0,dataAsOf:date}});
    return route.fulfill({json:{items:[]}});
  });
  await page.goto('/suppliers/target');
  await expect(page.getByRole('alert')).toContainText('不存在或无访问权限');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  allow=true;
  await page.getByRole('button',{name:'重试详情'}).click();
  await expect(page.getByRole('dialog',{name:'供应商详情'})).toBeVisible();
});

for(const product of [false,true])test(`${product?'商品':'核心'}清除仅作用于当前类别`,async({page})=>{
  await page.route('**/api/**',async route=>{
    if(new URL(route.request().url()).pathname==='/api/session')return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    return route.fulfill({json:{items:[],total:0,canManage:true,dataAsOf:date}});
  });
  await page.goto(product?'/#productData':'/#coreData');
  const search=page.getByRole('textbox',{name:'检索',exact:true});
  const status=page.getByRole('combobox',{name:'状态',exact:true});
  const first=page.getByRole('tab').first();
  const other=page.getByRole('tab').nth(1);
  await search.fill('保留词');
  await other.click();
  await search.fill('当前词');
  await status.selectOption('disabled');
  await page.getByRole('button',{name:'清除筛选'}).click();
  await expect(search).toHaveValue('');
  await expect(status).toHaveValue('all');
  await expect(page.getByRole('button',{name:'清除筛选'})).toBeDisabled();
  await first.click();
  await expect(search).toHaveValue('保留词');
});

for(const [tab, fields] of [
  ['商品', ['商品分类 *','品牌','基础单位 *']],
  ['商品分类', ['上级分类']],
  ['SKU / 条码', ['所属商品 *']],
  ['单位换算', ['商品 *','换算单位 *']],
] as const)test(`${tab}关联选择按编码名称搜索、键盘选择和 Esc`,async({page})=>{
  const items=['product','product_category','brand','unit_of_measure'].flatMap(resourceType=>[1,2].map(n=>({id:`${resourceType}-${n}`,resourceType,code:`CODE-${n}`,name:`候选资料${n}`,status:'active',version:1,updatedAt:date,unitOfMeasureCode:'EA'})));
  await page.route('**/api/**',async route=>{
    if(new URL(route.request().url()).pathname==='/api/session')return route.fulfill({json:{authenticated:true,csrfToken:'csrf'}});
    return route.fulfill({json:{items,canManage:true,dataAsOf:date}});
  });
  await page.goto('/#productData');
  await page.getByRole('tab').filter({has:page.locator('span',{hasText:new RegExp(`^${tab.replace('/','\\/')}$`)})}).click();
  await page.getByRole('button',{name:`＋ 新增${tab}`,exact:true}).click();
  const dialog=page.getByRole('dialog',{name:`新增${tab}`,exact:true});
  for(const label of fields){
    const control=dialog.getByRole('combobox',{name:label,exact:true});
    await control.click();
    const search=dialog.getByRole('combobox',{name:`搜索${label}`,exact:true});
    await search.fill('没有这个候选');
    await expect(dialog.getByRole('status')).toContainText('没有匹配资料');
    await page.keyboard.press('Escape');
    await expect(dialog).toBeVisible();
    await expect(control).toBeFocused();
    await control.click();
    await search.fill('CODE-2');
    await dialog.getByRole('option',{name:'CODE-2 · 候选资料2',exact:true}).click();
    await expect(control).toContainText('候选资料2');
    await control.click();
    await search.fill('候选资料1');
    // Optional fields have an empty choice first; move to the matching record.
    if(!label.endsWith('*'))await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(control).toContainText('候选资料1');
  }
});
