import { expect, test } from "@playwright/test";

import { signInWithPasswordAndTotp } from "./authentikMfa";
import { installMockBridge } from "../helpers/bridge";

const AUTH_ORIGIN = "https://auth.bizfin.test";

test("Business IAM accepts the existing Authentik session without Step-up", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await installMockBridge(page);
  await page.route("https://business.bizfin.test/api/iam/**", async (route) => {
    const body = route.request().url().includes("change-requests")
      ? []
      : { principals: [], roles: [], permissions: [] };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      headers: {
        "access-control-allow-origin": "https://workbench.bizfin.test",
        "cache-control": "no-store",
      },
      body: JSON.stringify(body),
    });
  });

  await page.goto("/?e2e=mock&resetDevState=1");
  await page.getByRole("button", { name: "Sign in with Authentik" }).click();
  await signInWithPasswordAndTotp(page, AUTH_ORIGIN);
  await expect(page).toHaveURL(
    /^https:\/\/workbench\.bizfin\.test\/(?:\?e2e=mock)?$/,
  );
  await expect(page.getByTestId("workbench-auth-gate")).toBeHidden();

  await page.getByTestId("business-iam-admin-toggle").click();
  const dialog = page.getByTestId("business-iam-admin-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Step up" })).toHaveCount(0);
  await expect(dialog.getByRole("tab", { name: "Review queue" })).toBeVisible();
  await expect(dialog.getByText("No changes in this queue")).toBeVisible();
});
