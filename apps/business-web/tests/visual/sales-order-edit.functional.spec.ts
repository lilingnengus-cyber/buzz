import { expect, test } from "@playwright/test";

test("销售草稿回填、保存保留隐藏字段，冲突不关闭，取消不写入", async ({
  page,
}) => {
  const writes: any[] = [];
  const draft = {
    id: "draft",
    orderNumber: "SO-EDIT",
    legalEntityId: "legal",
    customerId: "customer",
    businessUnitId: "unit",
    departmentId: "department",
    brandId: "brand",
    currency: "USD",
    paymentTermsDays: 45,
    orderDate: "2026-10-01",
    requestedDeliveryDate: "2026-10-03",
    customerReference: "ORIGINAL",
    businessNote: "原备注",
    lifecycleStatus: "draft",
    version: 7,
    lines: [
      {
        skuId: "sku",
        warehouseId: "warehouse",
        unitOfMeasureId: "uom",
        quantity: "2",
        unitPrice: "100",
        discountAmount: "10",
        taxRate: "0.13",
        businessUnitId: "line-unit",
        departmentId: "line-dept",
        brandId: "line-brand",
      },
    ],
  };
  const ids: Record<string, string> = {
    legal_entity: "legal",
    customer: "customer",
    sku: "sku",
    warehouse: "warehouse",
    unit_of_measure: "uom",
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
    else if (req.method() === "PUT" && path === "/api/v1/sales-orders/draft") {
      writes.push(req.postDataJSON());
      await route.fulfill(
        writes.length === 1
          ? {
              status: 409,
              json: {
                code: "VERSION_CONFLICT",
                message: "订单版本已变化，请刷新后重试。",
              },
            }
          : { json: { number: "SO-EDIT", version: 8 } },
      );
    } else if (path.endsWith("/draft-options"))
      await route.fulfill({ json: { canUpdate: true, draft } });
    else if (path === "/api/v1/sales-orders")
      await route.fulfill({
        json: {
          items: [
            {
              ...draft,
              holdStatus: "none",
              fulfillmentStatus: "unreserved",
              grossAmount: "214.7",
              updatedAt: "2026-10-01T00:00:00Z",
            },
          ],
        },
      });
    else if (path.startsWith("/api/v1/master-data/")) {
      const type = path.split("/").at(-1)!;
      await route.fulfill({
        json: {
          items: [
            {
              id: ids[type],
              resourceType: type,
              code: type,
              name: type,
              status: "active",
              legalEntityId: null,
            },
          ],
        },
      });
    } else if (path === "/api/v1/core-master-data")
      await route.fulfill({
        json: {
          items: [
            {
              id: "unit",
              resourceType: "business_unit",
              code: "BU",
              name: "经营单元",
              status: "active",
              parentBusinessUnitId: null,
              ancestorPath: ["经营单元"],
              depth: 0,
              descendantCount: 0,
            },
          ],
        },
      });
    else await route.fulfill({ json: { items: [] } });
  });
  await page.goto("/#sales");
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "SO-EDIT · 编辑草稿" });
  await expect(
    dialog.getByRole("combobox", { name: "法律主体" }),
  ).toBeDisabled();
  await expect(
    dialog.getByRole("spinbutton", { name: "第 1 行税率 %" }),
  ).toHaveValue("13");
  await expect(dialog.getByRole("textbox", { name: "客户参考号" })).toHaveValue(
    "ORIGINAL",
  );
  await dialog.getByRole("textbox", { name: "客户参考号" }).fill("UPDATED");
  await dialog.getByRole("button", { name: "保存修改" }).click();
  await expect(dialog).toContainText("订单版本已变化");
  expect(writes[0]).toMatchObject({
    expectedVersion: 7,
    currency: "USD",
    paymentTermsDays: 45,
    brandId: "brand",
    departmentId: "department",
    customerReference: "UPDATED",
    lines: [
      {
        taxRate: "0.13",
        businessUnitId: "line-unit",
        departmentId: "line-dept",
        brandId: "line-brand",
      },
    ],
  });
  expect(writes[0]).not.toHaveProperty("legalEntityId");
  await dialog.getByRole("button", { name: "保存修改" }).click();
  await expect(dialog).toHaveCount(0);
  await page.getByRole("button", { name: "编辑", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "保存修改" })).toBeVisible();
  await dialog.getByRole("button", { name: "关闭弹窗", exact: true }).click();
  expect(writes).toHaveLength(2);
});
