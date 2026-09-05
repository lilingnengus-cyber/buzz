import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";

const AGENT =
  "a110000000000000000000000000000000000000000000000000000000000042";
const TRACE = "123e4567-e89b-42d3-a456-426614174000";
const AUDIT = "123e4567-e89b-42d3-a456-426614174001";
const lifeSessionStates = new WeakMap<
  Page,
  {
    embedCount: number;
    nextWorkbenchTtlMs: number;
    workbenchCount: number;
  }
>();

function oidcFixtureToken() {
  const encode = (value: object) =>
    Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${encode({ alg: "RS256", kid: "fixture" })}.${encode({ nonce: "life-e2e-nonce" })}.signature`;
}

async function waitForMockLiveSubscription(page: Page, channelName: string) {
  await expect
    .poll(() =>
      page.evaluate(
        (name) =>
          window.__BUZZ_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
            channelName: name,
            kind: 9,
            exactChannel: true,
          }) ?? false,
        channelName,
      ),
    )
    .toBe(true);
}

test.describe("Life Dock", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.addInitScript((token) => {
      window.__BUZZ_E2E_LIFE_ACCESS_TOKEN__ = token;
    }, oidcFixtureToken());
    const sessionState = {
      embedCount: 0,
      nextWorkbenchTtlMs: 10 * 60_000,
      workbenchCount: 0,
    };
    const bindingPayload = `life-workbench-identity-binding-v1\nfixture\nissued_at=${Math.floor(Date.now() / 1000)}`;
    lifeSessionStates.set(page, sessionState);
    await page.route("**/v1/workbench/sessions", (route) => {
      sessionState.workbenchCount += 1;
      const ttlMs = sessionState.nextWorkbenchTtlMs;
      sessionState.nextWorkbenchTtlMs = 10 * 60_000;
      return route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          sessionId: "123e4567-e89b-42d3-a456-426614174010",
          sessionToken: "S".repeat(43),
          expiresAt: new Date(Date.now() + ttlMs).toISOString(),
        }),
      });
    });
    await page.route("**/v1/embed-sessions", (route) => {
      sessionState.embedCount += 1;
      return route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          embedSessionId: `123e4567-e89b-42d3-a456-42661417401${sessionState.embedCount}`,
          embedUrl: `http://127.0.0.1:4173/embed/bootstrap?code=${"C".repeat(43)}`,
          expiresAt: new Date(Date.now() + 60_000).toISOString(),
          traceId: TRACE,
        }),
      });
    });
    await page.route("**/v1/me", (route) =>
      route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          userId: "123e4567-e89b-42d3-a456-426614174020",
          lifeOsUserId: "default-user",
          status: "active",
          sessionId: "123e4567-e89b-42d3-a456-426614174010",
          deploymentId: "life-production",
          memberships: [],
          bindings: [],
        }),
      }),
    );
    await page.route("**/v1/identity-bindings/challenges", (route) =>
      route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          challengeId: "123e4567-e89b-42d3-a456-426614174021",
          audience: "life-workbench-identity-binding",
          canonicalPayload: bindingPayload,
          expiresAt: new Date(Date.now() + 60_000).toISOString(),
          traceId: TRACE,
        }),
      }),
    );
    await page.route("**/v1/identity-bindings", async (route) => {
      const body = route.request().postDataJSON() as {
        challengeId?: string;
        signedEvent?: { kind?: number; content?: string; pubkey?: string };
      };
      expect(body.challengeId).toBe("123e4567-e89b-42d3-a456-426614174021");
      expect(body.signedEvent).toMatchObject({
        kind: 24243,
        content: bindingPayload,
        pubkey: "deadbeef".repeat(8),
      });
      return route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          bindingId: "123e4567-e89b-42d3-a456-426614174022",
          pubkey: "deadbeef".repeat(8),
          status: "active",
          createdAt: new Date().toISOString(),
          version: 1,
        }),
      });
    });
    await page.route("**/embed/bootstrap?code=*", (route) =>
      route.fulfill({
        status: 303,
        headers: { location: "/life-dock-test.html?bootstrap=1" },
      }),
    );
    await installMockBridge(page, {
      managedAgents: [
        {
          pubkey: AGENT,
          name: "Life Agent",
          status: "running",
          channelNames: ["general"],
        },
      ],
    });
    await page.goto("/");
    await expect(page.getByTestId("home-inbox-list")).toBeVisible();
  });

  test("bootstraps, navigates, guards dirty state, pins, and keeps the iframe mounted", async ({
    page,
  }) => {
    await page.getByTestId("life-dock-toggle").click();
    const dock = page.getByTestId("life-dock");
    const frame = page.frameLocator('[data-testid="life-dock-iframe"]');
    await expect(dock).toBeVisible();
    const sessionState = lifeSessionStates.get(page);
    if (!sessionState)
      throw new Error("Life session counter was not installed");
    await expect.poll(() => sessionState.embedCount).toBe(1);
    await expect(
      frame.getByRole("heading", { name: "LifeOS Dock Mock" }),
    ).toBeVisible();
    await expect(frame.locator("#bootstrap")).toHaveText("redeemed");
    await expect(frame.locator("#bridge")).toHaveText("connected-v2");
    await expect(frame.locator("#theme")).not.toHaveText("unknown");
    const instance = await frame.locator("#instance").textContent();

    await page.getByTestId("channel-general").click();
    await waitForMockLiveSubscription(page, "general");
    await page.evaluate(() => {
      const testWindow = window as Window & {
        __LIFE_E2E_CANDIDATES__?: unknown[];
      };
      testWindow.__LIFE_E2E_CANDIDATES__ = [];
      window.addEventListener("buzz:trusted-life-result-candidate", (event) => {
        testWindow.__LIFE_E2E_CANDIDATES__?.push((event as CustomEvent).detail);
      });
    });
    // React StrictMode tears down its first subscription asynchronously. Wait
    // past the relay client's 250ms readiness fallback so the mock cannot
    // mistake that disposable subscription for the live replacement.
    await page.waitForTimeout(300);
    const emitted = await page.evaluate(
      ({ agent, trace, audit }) => {
        return window.__BUZZ_E2E_EMIT_MOCK_MESSAGE__?.({
          channelName: "general",
          content: "已完成受信 LifeOS 操作。",
          pubkey: agent,
          kind: 9,
          createdAt: Math.floor(Date.now() / 1000),
          extraTags: [
            [
              "pacioli-extension-result",
              "1",
              "life",
              "action.status.update",
              "succeeded",
              trace,
              audit,
            ],
            [
              "pacioli-resource-ref",
              "1",
              trace,
              "life://action/trusted-action",
              "8",
              "可信行动",
            ],
          ],
        });
      },
      { agent: AGENT, trace: TRACE, audit: AUDIT },
    );
    expect(emitted?.kind).toBe(9);
    expect(emitted?.tags).toContainEqual([
      "pacioli-extension-result",
      "1",
      "life",
      "action.status.update",
      "succeeded",
      TRACE,
      AUDIT,
    ]);
    await expect(page.getByText("已完成受信 LifeOS 操作。")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            (window as Window & { __LIFE_E2E_CANDIDATES__?: unknown[] })
              .__LIFE_E2E_CANDIDATES__?.length ?? 0,
        ),
      )
      .toBe(1);
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: trusted-action",
    );

    await page.evaluate((agent) => {
      window.__BUZZ_E2E_EMIT_MOCK_MESSAGE__?.({
        channelName: "general",
        content: "普通链接：[下一个行动](life://action/next-action)",
        pubkey: agent,
        createdAt: Math.floor(Date.now() / 1000),
      });
    }, AGENT);
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: trusted-action",
    );
    const nextActionLink = page.getByRole("link", { name: "下一个行动" });
    await expect(nextActionLink).toHaveAttribute(
      "href",
      "life://action/next-action",
    );
    await nextActionLink.click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: next-action",
    );
    await page.getByLabel("LifeOS back").click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: trusted-action",
    );
    await page.getByLabel("LifeOS forward").click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: next-action",
    );

    await frame.getByRole("button", { name: "Mark Life Dirty" }).click();
    await page.getByLabel("LifeOS home").click();
    const dialog = page.getByTestId("life-dock-dirty-dialog");
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "取消" }).click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: next-action",
    );
    await page.getByLabel("LifeOS home").click();
    await dialog.getByRole("button", { name: "仍然离开" }).click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "dashboard",
    );

    await page.getByLabel("Pin Life Dock").click();
    await expect(page.getByLabel("Unpin Life Dock")).toBeVisible();
    await waitForAnimations(page);
    await dock.screenshot({ path: "test-results/life-dock/01-life-dock.png" });
    await page.getByTestId("business-dock-toggle").click();
    await expect(page.getByTestId("business-dock")).toBeVisible();
    await expect(dock).toBeHidden();
    await page.getByTestId("life-dock-toggle").click();
    await expect(dock).toBeVisible();
    await expect(frame.locator("#instance")).toHaveText(instance ?? "");
    await waitForAnimations(page);
    await dock.screenshot({
      path: "test-results/life-dock/02-life-dock-restored.png",
    });
  });

  test("renews a near-expiry session without reloading the iframe or losing dirty state", async ({
    page,
  }) => {
    const sessionState = lifeSessionStates.get(page);
    if (!sessionState)
      throw new Error("Life session counter was not installed");
    sessionState.nextWorkbenchTtlMs = 94_000;

    await page.getByTestId("life-dock-toggle").click();
    const frame = page.frameLocator('[data-testid="life-dock-iframe"]');
    await expect(frame.locator("#bootstrap")).toHaveText("redeemed");
    await frame.getByRole("button", { name: "Open action fixture" }).click();
    await frame.getByRole("button", { name: "Mark Life Dirty" }).click();
    const instance = await frame.locator("#instance").textContent();

    await expect
      .poll(() => sessionState.embedCount, { timeout: 10_000 })
      .toBe(2);
    expect(sessionState.workbenchCount).toBe(2);
    await expect(frame.locator("#renewal-count")).toHaveText("1");
    await expect(frame.locator("#instance")).toHaveText(instance ?? "");
    await expect(frame.locator("#current-resource")).toHaveText(
      "action · fixture-action",
    );

    await page.getByLabel("LifeOS home").click();
    await expect(page.getByTestId("life-dock-dirty-dialog")).toBeVisible();
  });

  test("ignores wrong-source messages and performs one recovery attempt per expiry", async ({
    page,
  }) => {
    await page.getByTestId("channel-general").click();
    await waitForMockLiveSubscription(page, "general");
    await page.getByTestId("life-dock-toggle").click();
    await expect(page.getByTestId("life-dock")).toBeVisible();
    const sessionState = lifeSessionStates.get(page);
    if (!sessionState)
      throw new Error("Life session counter was not installed");
    await expect.poll(() => sessionState.embedCount).toBe(1);
    const frame = page.frameLocator('[data-testid="life-dock-iframe"]');
    await expect(frame.locator("#bootstrap")).toHaveText("redeemed");
    await frame.getByRole("button", { name: "Open action fixture" }).click();
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: fixture-action",
    );
    const nonce = await frame
      .locator("body")
      .evaluate(() => window.__LIFE_FIXTURE_NONCE__);
    await page.evaluate(
      ({ nonce, trace }) => {
        window.postMessage(
          {
            version: 2,
            type: "RESOURCE_CHANGED",
            requestId: "forged",
            sessionNonce: nonce,
            payload: {
              resource: {
                version: 1,
                extensionId: "life",
                type: "action",
                id: "forged-action",
                path: "/embed/actions/forged-action",
              },
              traceId: trace,
            },
          },
          window.origin,
        );
      },
      { nonce, trace: TRACE },
    );
    await expect(page.getByTestId("life-resource-label")).toHaveText(
      "action: fixture-action",
    );

    await frame.getByRole("button", { name: "Expire Life Session" }).click();
    await expect(frame.locator("#bootstrap")).toHaveText("redeemed");
    await expect(page.getByTestId("life-auth-required")).toBeHidden();
    await expect.poll(() => sessionState.embedCount).toBe(2);
    await page.waitForTimeout(250);
    expect(sessionState.embedCount).toBe(2);
  });
});
