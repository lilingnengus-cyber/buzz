import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../../../../desktop/tests/helpers/animations";

test("独立销售录单、混合订单分流和共用收款按来源核销", async ({ page }) => {
  const requests: string[] = [];
  const base = {
    legalEntityId: "le",
    customerId: "customer",
    salesOrderId: "mixed",
    currency: "CNY",
    originalAmount: "100",
    settledAmount: "0",
    openAmount: "100",
    dueDate: "2026-11-01",
    status: "open",
    isOverdue: false,
    overdueDays: 0,
    updatedAt: "2026-10-03T00:00:00Z",
    version: 1,
  };
  const ars = [
    {
      ...base,
      id: "ar-g",
      receivableNumber: "AR-GOODS",
      shipmentId: "shipment",
      serviceProjectId: null,
    },
    {
      ...base,
      id: "ar-s",
      receivableNumber: "AR-SERVICE",
      shipmentId: null,
      serviceProjectId: "project",
    },
  ];
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    requests.push(url.pathname + url.search);
    if (url.pathname === "/api/session")
      return route.fulfill({
        json: { authenticated: true, csrfToken: "csrf", displayName: "测试" },
      });
    if (url.pathname === "/api/v1/trade-receivables")
      return route.fulfill({ json: { items: ars } }); // Defensively filter even a broad response.
    if (url.pathname === "/api/v1/customer-receipts")
      return route.fulfill({
        json: {
          items: [
            {
              id: "receipt",
              receiptNumber: "RCPT-SHARED",
              legalEntityId: "le",
              customerId: "customer",
              currency: "CNY",
              receiptDate: "2026-10-03",
              amount: "200",
              allocatedAmount: "0",
              unappliedAmount: "200",
              status: "confirmed",
              version: 1,
            },
          ],
        },
      });
    if (url.pathname === "/api/v1/shipments/draft-options")
      return route.fulfill({
        json: {
          canCreate: true,
          items: [
            {
              salesOrderLineId: "goods-line",
              orderId: "mixed",
              orderNumber: "SO-MIXED",
              customerName: "混合客户",
              skuName: "设备",
              shippableQuantity: "1",
            },
          ],
        },
      });
    if (url.pathname === "/api/v1/service-project-options")
      return route.fulfill({
        json: {
          items: [],
          owners: [],
          orderLines: [
            {
              id: "service-line",
              order_number: "SO-MIXED",
              customer_name: "混合客户",
              title: "实施服务",
              amount: "100",
              currency: "CNY",
            },
          ],
        },
      });
    return route.fulfill({
      json: { items: [], hasMore: false, canManage: true },
    });
  });
  await page.goto("/#sales");
  await expect(
    page.getByRole("heading", { name: "销售订单", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "新增销售订单" }),
  ).toBeVisible();
  await expect(
    page.getByRole("navigation", { name: "订单闭环阶段" }),
  ).toHaveCount(0);
  expect(requests.some((p) => p.includes("trade-receivables"))).toBeFalsy();
  const nav = page.getByRole("navigation", { name: "业务导航" });
  await expect(
    nav.getByRole("link", { name: "服务项目", exact: true }),
  ).toHaveCount(0);
  await nav.getByRole("link", { name: "商品订单闭环", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "商品订单闭环", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "新增销售订单" })).toHaveCount(
    0,
  );
  await page
    .locator("summary")
    .filter({ hasText: "待出库的商品订单行" })
    .click();
  await expect(
    page.getByRole("cell", { name: "设备", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /商品应收/ }).click();
  await expect(page.getByText("AR-GOODS", { exact: true })).toBeVisible();
  await expect(page.getByText("AR-SERVICE", { exact: true })).toHaveCount(0);
  expect(requests.some((p) => p.includes("sourceKind=goods"))).toBeTruthy();
  await nav.getByRole("link", { name: "服务订单闭环", exact: true }).click();
  await page
    .locator("summary")
    .filter({ hasText: "待创建项目的服务订单行" })
    .click();
  await expect(
    page.getByRole("cell", { name: "实施服务", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: /服务应收/ }).click();
  await expect(page.getByText("AR-SERVICE", { exact: true })).toBeVisible();
  await expect(page.getByText("AR-GOODS", { exact: true })).toHaveCount(0);
  expect(requests.some((p) => p.includes("sourceKind=service"))).toBeTruthy();
  await page.reload();
  await expect(page.getByText("AR-SERVICE", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /收款核销/ }).click();
  await page.getByRole("button", { name: "核销", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("textbox", { name: "AR-SERVICE 核销金额" }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("textbox", { name: "AR-GOODS 核销金额" }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /交付事项/ }).click();
  await expect(
    page.getByRole("heading", { name: "交付事项", exact: true }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ path: "test-results/service-order-workflow.png" });
  await page.goto("/#serviceDeliverables");
  await expect(
    page.getByRole("heading", { name: "服务订单闭环", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "交付事项", exact: true }),
  ).toBeVisible();
});
