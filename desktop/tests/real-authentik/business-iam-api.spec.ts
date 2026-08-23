import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";
import {
  readWorkbenchAccessToken,
  readWorkbenchAccessTokenClaims,
  signInWithPasswordAndTotp,
} from "./authentikMfa";

const AUTH_ORIGIN = "https://auth.bizfin.test";

test("real Authentik MFA token is accepted but cannot exceed Business IAM", async ({
  page,
}) => {
  const adminApi = process.env.BUSINESS_IAM_ADMIN_E2E_URL;
  if (!adminApi)
    throw new Error("BUSINESS_IAM_ADMIN_E2E_URL is required for this test");

  await installMockBridge(page);
  await page.goto("/?e2e=mock&resetDevState=1");
  await page.getByRole("button", { name: "Sign in with Authentik" }).click();
  await signInWithPasswordAndTotp(page, AUTH_ORIGIN);
  await expect(page).toHaveURL(
    /^https:\/\/workbench\.bizfin\.test\/(?:\?e2e=mock)?$/,
  );
  const claims = await readWorkbenchAccessTokenClaims(page);
  expect(claims.issuer).toBe(
    "https://auth.bizfin.test/application/o/workbench/",
  );
  if (process.env.POC_USER_OIDC_SUBJECT)
    expect(claims.subject).toBe(process.env.POC_USER_OIDC_SUBJECT);
  const token = await readWorkbenchAccessToken(page);

  const catalog = await page.request.get(`${adminApi}/api/iam/catalog`, {
    headers: { authorization: `Bearer ${token}` },
  });
  const catalogText = await catalog.text();
  expect(catalog.status(), catalogText).toBe(200);
  expect(catalog.headers()["cache-control"]).toBe("no-store");
  const catalogBody = JSON.parse(catalogText);
  expect(catalogBody.principals).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        kind: "human",
        displayName: "POC User",
      }),
    ]),
  );

  const overreach = await page.request.post(
    `${adminApi}/api/iam/change-requests`,
    {
      headers: { authorization: `Bearer ${token}` },
      data: {
        operation: "permission_grant",
        payload: {
          externalId: "missing-agent",
          capability: "sales_order:read",
          dataScope: { mode: "unrestricted" },
          obligations: [],
          expectedVersion: 1,
        },
        reason: "This read-only administrator must not request grants",
        idempotencyKey: "real-authentik-overreach-v1",
      },
    },
  );
  expect(overreach.status()).toBe(403);
  await expect(overreach.json()).resolves.toMatchObject({
    error: "business_iam_permission_denied",
  });
});
