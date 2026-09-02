import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

type LifeWorkbenchSession = {
  sessionId: string;
  sessionToken: string;
  expiresAt: string;
};

export type IssuedLifeEmbedSession = {
  embedSessionId: string;
  embedUrl: string;
  expiresAt: string;
  traceId: string;
};

const TOKEN_PATTERN = /^[A-Za-z0-9_-]{43}$/u;
const SAFE_ID_PATTERN = /^[A-Za-z0-9-]{1,128}$/u;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function tokenNonce(token: string): string | null {
  if (!token || token.length > 16 * 1024) return null;
  const payload = token.split(".")[1];
  if (!payload) return null;
  try {
    const normalized = payload.replaceAll("-", "+").replaceAll("_", "/");
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
    const decoded = JSON.parse(atob(padded)) as unknown;
    return isRecord(decoded) &&
      typeof decoded.nonce === "string" &&
      decoded.nonce.length > 0 &&
      decoded.nonce.length <= 512
      ? decoded.nonce
      : null;
  } catch {
    return null;
  }
}

async function readError(response: Response): Promise<string> {
  const body = (await response.json().catch(() => null)) as unknown;
  return isRecord(body) && typeof body.error === "string"
    ? body.error
    : `Life Gateway request failed (${response.status})`;
}

async function postJson(
  gateway: string,
  path: string,
  authorization: string,
  body: object,
): Promise<unknown> {
  const target = new URL(path, gateway);
  if (target.origin !== gateway)
    throw new Error("Life Gateway origin mismatch.");
  const response = await fetch(target, {
    method: "POST",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    headers: {
      Authorization: authorization,
      "Content-Type": "application/json",
      "X-Trace-Id": crypto.randomUUID(),
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(await readError(response));
  return response.status === 204 ? null : response.json();
}

/** Exchanges a verified Workbench OIDC token for an isolated Life session. */
export async function createLifeWorkbenchSession(
  gateway: string,
  oidcToken: string,
): Promise<LifeWorkbenchSession> {
  const nonce = tokenNonce(oidcToken);
  if (!nonce) throw new Error("Workbench OIDC nonce is unavailable.");
  const value = await postJson(
    gateway,
    "/v1/workbench/sessions",
    `Bearer ${oidcToken}`,
    { nonce },
  );
  if (
    !isRecord(value) ||
    typeof value.sessionId !== "string" ||
    !SAFE_ID_PATTERN.test(value.sessionId) ||
    typeof value.sessionToken !== "string" ||
    !TOKEN_PATTERN.test(value.sessionToken) ||
    typeof value.expiresAt !== "string"
  ) {
    throw new Error("Life Gateway returned an invalid Workbench session.");
  }
  return {
    sessionId: value.sessionId,
    sessionToken: value.sessionToken,
    expiresAt: value.expiresAt,
  };
}

/** Issues one LifeOS bootstrap URL bound to the canonical resource path. */
export async function issueLifeEmbedSession(
  gateway: string,
  sessionToken: string,
  resource: WorkspaceResource,
): Promise<IssuedLifeEmbedSession> {
  if (!TOKEN_PATTERN.test(sessionToken) || resource.extensionId !== "life") {
    throw new Error("Life Workbench session is invalid.");
  }
  const value = await postJson(
    gateway,
    "/v1/embed-sessions",
    `Bearer ${sessionToken}`,
    { targetPath: resource.path },
  );
  if (
    !isRecord(value) ||
    typeof value.embedSessionId !== "string" ||
    !SAFE_ID_PATTERN.test(value.embedSessionId) ||
    typeof value.embedUrl !== "string" ||
    typeof value.expiresAt !== "string" ||
    typeof value.traceId !== "string" ||
    !SAFE_ID_PATTERN.test(value.traceId)
  ) {
    throw new Error("Life Gateway returned an invalid Embed Session.");
  }
  return {
    embedSessionId: value.embedSessionId,
    embedUrl: value.embedUrl,
    expiresAt: value.expiresAt,
    traceId: value.traceId,
  };
}

export async function revokeLifeEmbedSession(
  gateway: string,
  sessionToken: string,
  embedSessionId: string,
): Promise<void> {
  if (
    !TOKEN_PATTERN.test(sessionToken) ||
    !SAFE_ID_PATTERN.test(embedSessionId)
  )
    return;
  await postJson(
    gateway,
    `/v1/embed-sessions/${embedSessionId}/revoke`,
    `Bearer ${sessionToken}`,
    {},
  ).catch(() => undefined);
}
