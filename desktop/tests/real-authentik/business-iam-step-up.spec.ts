import { expect, test } from "@playwright/test";

import {
  readWorkbenchAccessTokenClaims,
  signInWithPasswordAndTotp,
} from "./authentikMfa";
import { installMockBridge } from "../helpers/bridge";

const AUTH_ORIGIN = "https://auth.bizfin.test";

test("Business IAM Step-up requires a fresh Authentik password and TOTP event", async ({
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
  const initialClaims = await readWorkbenchAccessTokenClaims(page);

  await page.getByTestId("business-iam-admin-toggle").click();
  const dialog = page.getByTestId("business-iam-admin-dialog");
  await expect(dialog).toBeVisible();
  // Authentik rejects replaying the same TOTP counter, so cross a period
  // boundary before proving the second independent validation event.
  await page.waitForTimeout(30_500 - (Date.now() % 30_000));
  await dialog.getByRole("button", { name: "Step up" }).click();
  await expect(page).toHaveURL(/prompt=login/);
  await expect(page).toHaveURL(/max_age=0/);
  await signInWithPasswordAndTotp(page, AUTH_ORIGIN);

  await expect(page).toHaveURL(
    /^https:\/\/workbench\.bizfin\.test\/(?:\?e2e=mock)?$/,
  );
  const steppedUpClaims = await readWorkbenchAccessTokenClaims(page);
  expect(steppedUpClaims.authTime).toBeGreaterThan(initialClaims.authTime);
});
