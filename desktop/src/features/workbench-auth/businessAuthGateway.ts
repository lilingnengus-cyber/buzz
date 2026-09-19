export type EnterpriseUserSummary = {
  id: string;
  email?: string | null;
  displayName: string;
  status: "active" | "disabled";
};

export type BuzzIdentityBinding = {
  id: string;
  buzzPubkey: string;
  deviceId?: string | null;
  deviceName?: string | null;
  devicePlatform?: "macos" | "windows" | "linux" | "web" | null;
  status: "active" | "revoked";
  boundAt: string;
  lastSeenAt: string;
  revokedAt?: string | null;
  version: number;
};

export type GatewayMe = {
  user: EnterpriseUserSummary;
  workbenchSessionId: string;
  bindings: BuzzIdentityBinding[];
};

export type WorkbenchAuthState =
  | { status: "initializing" }
  | { status: "unauthenticated" }
  | { status: "authenticating" }
  | {
      status: "authenticated";
      user: EnterpriseUserSummary;
      workbenchSessionId: string;
    }
  | { status: "error"; error: string };

export function getBusinessAuthGatewayUrl(): string | null {
  const value = import.meta.env.VITE_BUSINESS_AUTH_GATEWAY_URL?.trim();
  if (!value) return null;
  try {
    const url = new URL(value);
    if (!["https:", "http:"].includes(url.protocol) || url.pathname !== "/")
      return null;
    return url.origin;
  } catch {
    return null;
  }
}

async function gatewayFetch<T>(
  gateway: string,
  token: string,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const response = await fetch(new URL(path, gateway), {
    ...init,
    cache: "no-store",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
      "X-Trace-Id": crypto.randomUUID(),
      ...init.headers,
    },
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: string;
    } | null;
    throw new Error(
      body?.error ?? `Gateway request failed (${response.status})`,
    );
  }
  return (response.status === 204 ? undefined : await response.json()) as T;
}

export async function readGatewayState(
  gateway: string,
  token: string,
  proof?: {
    pubkey: string;
    issuer: string;
    subject: string;
    isCurrent: () => boolean;
    sign: (input: {
      kind: number;
      content: string;
      tags: string[][];
    }) => Promise<{ pubkey: string; content: string }>;
  },
): Promise<WorkbenchAuthState> {
  const me = await gatewayFetch<GatewayMe>(gateway, token, "/api/me");
  if (proof) {
    const assertCurrent = () => {
      if (!proof.isCurrent()) throw new Error("Workbench session changed.");
    };
    assertCurrent();
    if (me.user.status !== "active")
      throw new Error("Enterprise account is disabled.");
    const bindings = me.bindings.filter((b) => b.buzzPubkey === proof.pubkey);
    if (!bindings.some((b) => b.status === "active")) {
      // A refresh must never silently undo an administrator's revocation.
      if (bindings.length > 0)
        throw new Error(
          "Chat identity binding was revoked. Contact your administrator.",
        );
      if (!/^[0-9a-f]{64}$/.test(proof.pubkey))
        throw new Error("Chat identity is unavailable.");
      const challenge = await gatewayFetch<{
        id: string;
        audience: string;
        payload: string;
        expiresAt: string;
      }>(gateway, token, "/api/identity-bindings/challenges", {
        method: "POST",
        body: JSON.stringify({ pubkey: proof.pubkey }),
      });
      assertCurrent();
      const lines = challenge.payload.split("\n");
      const issuedAt = Number(lines[7]?.slice("issued_at=".length));
      const expiresAt = Number(lines[8]?.slice("expires_at=".length));
      if (
        challenge.audience !== "bizfin-workbench-identity-binding" ||
        lines.length !== 9 ||
        lines[0] !== "bizfin-identity-binding-v1" ||
        lines[1] !== `challenge_id=${challenge.id}` ||
        !/^nonce=[A-Za-z0-9_-]+$/.test(lines[2] ?? "") ||
        lines[3] !== `audience=${challenge.audience}` ||
        lines[4] !== `oidc_issuer=${proof.issuer}` ||
        lines[5] !== `oidc_subject=${proof.subject}` ||
        lines[6] !== `buzz_pubkey=${proof.pubkey}` ||
        !/^issued_at=\d+$/.test(lines[7] ?? "") ||
        !/^expires_at=\d+$/.test(lines[8] ?? "") ||
        !Number.isSafeInteger(issuedAt) ||
        !Number.isSafeInteger(expiresAt) ||
        issuedAt > Date.now() / 1000 + 60 ||
        issuedAt >= expiresAt ||
        expiresAt <= Date.now() / 1000 ||
        Math.floor(Date.parse(challenge.expiresAt) / 1000) !== expiresAt
      )
        throw new Error("Invalid enterprise identity challenge.");
      const signedEvent = await proof.sign({
        kind: 24243,
        content: challenge.payload,
        tags: [],
      });
      assertCurrent();
      if (
        signedEvent.pubkey !== proof.pubkey ||
        signedEvent.content !== challenge.payload
      )
        throw new Error("Chat identity changed during binding.");
      const binding = await gatewayFetch<BuzzIdentityBinding>(
        gateway,
        token,
        "/api/identity-bindings/verify",
        {
          method: "POST",
          body: JSON.stringify({ challengeId: challenge.id, signedEvent }),
        },
      );
      assertCurrent();
      if (binding.status !== "active" || binding.buzzPubkey !== proof.pubkey)
        throw new Error("Enterprise identity binding was not verified.");
    }
  }
  return {
    status: "authenticated",
    user: me.user,
    workbenchSessionId: me.workbenchSessionId,
  };
}

export async function issueEmbedSession(
  gateway: string,
  token: string,
  target: { type: string; id: string; path: string },
): Promise<{ id: string; embedUrl: string; traceId: string }> {
  return gatewayFetch(gateway, token, "/api/embed-sessions", {
    method: "POST",
    body: JSON.stringify({ target }),
  });
}

export async function logoutWorkbenchSession(
  gateway: string,
  token: string,
  global: boolean,
): Promise<{ logoutUrl?: string } | undefined> {
  return gatewayFetch(
    gateway,
    token,
    global ? "/api/logout/global" : "/api/logout/workbench",
    { method: "POST", body: "{}" },
  );
}
