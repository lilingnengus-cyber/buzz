import { createHmac } from "node:crypto";
import { expect, type Page } from "@playwright/test";

function currentTotp(hexKey: string): string {
  if (!/^[0-9a-f]{40,}$/i.test(hexKey))
    throw new Error("POC_USER_TOTP_KEY must be at least 20 bytes of hex");
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30_000)));
  const digest = createHmac("sha1", Buffer.from(hexKey, "hex"))
    .update(counter)
    .digest();
  const offset = digest[digest.length - 1] & 0x0f;
  const binary =
    ((digest[offset] & 0x7f) << 24) |
    ((digest[offset + 1] & 0xff) << 16) |
    ((digest[offset + 2] & 0xff) << 8) |
    (digest[offset + 3] & 0xff);
  return (binary % 1_000_000).toString().padStart(6, "0");
}

export async function signInWithPasswordAndTotp(
  page: Page,
  authOrigin: string,
) {
  const username = process.env.POC_USER_USERNAME ?? "poc-user";
  const password = process.env.POC_USER_PASSWORD;
  const totpKey = process.env.POC_USER_TOTP_KEY;
  if (!password) throw new Error("POC_USER_PASSWORD is required");
  if (!totpKey) throw new Error("POC_USER_TOTP_KEY is required");

  await expect(page).toHaveURL(new RegExp(`^${authOrigin}`));
  const usernameInput = page.locator('input[name="uidField"]').first();
  await expect(usernameInput).toBeVisible();
  await usernameInput.fill(username);
  await page.getByRole("button", { name: /log in|continue/i }).click();

  const passwordInput = page.getByRole("textbox", { name: "Password" });
  await expect(passwordInput).toBeVisible();
  await passwordInput.fill(password);
  await page.getByRole("button", { name: "Continue" }).click();

  const totpInput = page.getByRole("textbox", {
    name: /time-based one-time password|authentication code/i,
  });
  await expect(totpInput).toBeVisible();
  await totpInput.fill(currentTotp(totpKey));
  await page.getByRole("button", { name: "Continue" }).click();
}

export async function readWorkbenchAccessTokenClaims(page: Page): Promise<{
  amr: string[];
  authTime: number;
  issuer: string;
  subject: string;
}> {
  const claims = await page.evaluate(() => {
    const stored = Object.entries(sessionStorage).find(([key]) =>
      key.startsWith("buzz.oidc.user."),
    );
    if (!stored) return null;
    const token = JSON.parse(stored[1]).access_token as string;
    const encoded = token
      .split(".")[1]
      .replaceAll("-", "+")
      .replaceAll("_", "/");
    const payload = JSON.parse(atob(encoded));
    return {
      authTime: payload.auth_time,
      amr: payload.amr,
      issuer: payload.iss,
      subject: payload.sub,
    };
  });
  expect(claims?.authTime).toEqual(expect.any(Number));
  expect(claims?.amr).toContain("mfa");
  expect(claims?.issuer).toEqual(expect.any(String));
  expect(claims?.subject).toEqual(expect.any(String));
  return claims as {
    amr: string[];
    authTime: number;
    issuer: string;
    subject: string;
  };
}

export async function readWorkbenchAccessToken(page: Page): Promise<string> {
  const token = await page.evaluate(() => {
    const stored = Object.entries(sessionStorage).find(([key]) =>
      key.startsWith("buzz.oidc.user."),
    );
    return stored ? (JSON.parse(stored[1]).access_token as string) : null;
  });
  expect(token).toEqual(expect.any(String));
  return token as string;
}
