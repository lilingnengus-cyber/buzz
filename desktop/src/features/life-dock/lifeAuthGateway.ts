import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

type LifeWorkbenchSession = {
  sessionId: string;
  sessionToken: string;
  expiresAt: string;
};

export type LifeIdentityBinding = {
  bindingId: string;
  pubkey: string;
  status: string;
  createdAt: string;
  version: number;
};

export type LifeWorkbenchAccount = {
  userId: string;
  lifeOsUserId: string;
  status: string;
  sessionId: string;
  deploymentId: string;
  bindings: LifeIdentityBinding[];
};

export type LifeIdentityBindingChallenge = {
  challengeId: string;
  audience: string;
  canonicalPayload: string;
  createdAt: number;
  expiresAt: string;
  traceId: string;
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

export function readOidcNonce(token: string): string | null {
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

export function readBindingIssuedAt(payload: string): number | null {
  const values = payload
    .split("\n")
    .filter((line) => line.startsWith("issued_at="))
    .map((line) => line.slice("issued_at=".length));
  if (values.length !== 1 || !/^\d{1,16}$/u.test(values[0] ?? "")) return null;
  const timestamp = Number(values[0]);
  return Number.isSafeInteger(timestamp) && timestamp > 0 ? timestamp : null;
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

async function getJson(
  gateway: string,
  path: string,
  authorization: string,
): Promise<unknown> {
  const target = new URL(path, gateway);
  if (target.origin !== gateway)
    throw new Error("Life Gateway origin mismatch.");
  const response = await fetch(target, {
    method: "GET",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    headers: {
      Authorization: authorization,
      "X-Trace-Id": crypto.randomUUID(),
    },
  });
  if (!response.ok) throw new Error(await readError(response));
  return response.json();
}

function readLifeIdentityBinding(value: unknown): LifeIdentityBinding | null {
  if (
    !isRecord(value) ||
    typeof value.bindingId !== "string" ||
    !SAFE_ID_PATTERN.test(value.bindingId) ||
    typeof value.pubkey !== "string" ||
    !/^[0-9a-f]{64}$/u.test(value.pubkey) ||
    typeof value.status !== "string" ||
    typeof value.createdAt !== "string" ||
    typeof value.version !== "number"
  )
    return null;
  return {
    bindingId: value.bindingId,
    pubkey: value.pubkey,
    status: value.status,
    createdAt: value.createdAt,
    version: value.version,
  };
}

/** Exchanges a verified Workbench OIDC token for an isolated Life session. */
export async function createLifeWorkbenchSession(
  gateway: string,
  oidcToken: string,
  idToken?: string | null,
): Promise<LifeWorkbenchSession> {
  const nonce = readOidcNonce(idToken ?? oidcToken);
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

/** Reads the mapped Life account and its current public-key bindings. */
export async function getLifeWorkbenchAccount(
  gateway: string,
  sessionToken: string,
): Promise<LifeWorkbenchAccount> {
  if (!TOKEN_PATTERN.test(sessionToken))
    throw new Error("Life Workbench session is invalid.");
  const value = await getJson(gateway, "/v1/me", `Bearer ${sessionToken}`);
  const bindings =
    isRecord(value) && Array.isArray(value.bindings)
      ? value.bindings.map(readLifeIdentityBinding)
      : [];
  if (
    !isRecord(value) ||
    typeof value.userId !== "string" ||
    !SAFE_ID_PATTERN.test(value.userId) ||
    typeof value.lifeOsUserId !== "string" ||
    value.lifeOsUserId.length === 0 ||
    typeof value.status !== "string" ||
    typeof value.sessionId !== "string" ||
    !SAFE_ID_PATTERN.test(value.sessionId) ||
    typeof value.deploymentId !== "string" ||
    value.deploymentId.length === 0 ||
    bindings.some((binding) => binding === null)
  ) {
    throw new Error("Life Gateway returned an invalid account.");
  }
  return {
    userId: value.userId,
    lifeOsUserId: value.lifeOsUserId,
    status: value.status,
    sessionId: value.sessionId,
    deploymentId: value.deploymentId,
    bindings: bindings as LifeIdentityBinding[],
  };
}

/** Creates a one-time challenge bound to this Life session and Nostr pubkey. */
export async function createLifeIdentityBindingChallenge(
  gateway: string,
  sessionToken: string,
  pubkey: string,
): Promise<LifeIdentityBindingChallenge> {
  if (!TOKEN_PATTERN.test(sessionToken) || !/^[0-9a-f]{64}$/u.test(pubkey))
    throw new Error("Life identity-binding request is invalid.");
  const value = await postJson(
    gateway,
    "/v1/identity-bindings/challenges",
    `Bearer ${sessionToken}`,
    { pubkey },
  );
  const createdAt =
    isRecord(value) && typeof value.canonicalPayload === "string"
      ? readBindingIssuedAt(value.canonicalPayload)
      : null;
  if (
    !isRecord(value) ||
    typeof value.challengeId !== "string" ||
    !SAFE_ID_PATTERN.test(value.challengeId) ||
    value.audience !== "life-workbench-identity-binding" ||
    typeof value.canonicalPayload !== "string" ||
    value.canonicalPayload.length === 0 ||
    value.canonicalPayload.length > 16 * 1024 ||
    typeof value.expiresAt !== "string" ||
    typeof value.traceId !== "string" ||
    !SAFE_ID_PATTERN.test(value.traceId) ||
    createdAt === null
  ) {
    throw new Error("Life Gateway returned an invalid binding challenge.");
  }
  return {
    challengeId: value.challengeId,
    audience: value.audience,
    canonicalPayload: value.canonicalPayload,
    createdAt,
    expiresAt: value.expiresAt,
    traceId: value.traceId,
  };
}

/** Submits the complete signed Nostr event and consumes its challenge once. */
export async function verifyLifeIdentityBinding(
  gateway: string,
  sessionToken: string,
  challengeId: string,
  signedEvent: object,
): Promise<LifeIdentityBinding> {
  if (!TOKEN_PATTERN.test(sessionToken) || !SAFE_ID_PATTERN.test(challengeId))
    throw new Error("Life identity-binding verification is invalid.");
  const value = await postJson(
    gateway,
    "/v1/identity-bindings",
    `Bearer ${sessionToken}`,
    { challengeId, signedEvent },
  );
  const binding = readLifeIdentityBinding(value);
  if (!binding)
    throw new Error("Life Gateway returned an invalid identity binding.");
  return binding;
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
