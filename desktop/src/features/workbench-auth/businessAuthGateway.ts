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
): Promise<WorkbenchAuthState> {
  const me = await gatewayFetch<GatewayMe>(gateway, token, "/api/me");
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
