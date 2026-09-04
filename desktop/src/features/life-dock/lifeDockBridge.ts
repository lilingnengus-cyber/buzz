import type { LifeDockConfig } from "./lifeDockConfig";
import { resolveLifeResource } from "./lifeResourceResolver";
import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

export type LifeBridgeEnvelope<T = unknown, V extends 2 | 3 = 2> = {
  version: V;
  type: string;
  requestId: string;
  sessionNonce: string;
  payload?: T;
};

export type LifeHostBridgeType =
  | "HOST_INIT"
  | "SET_THEME"
  | "REFRESH"
  | "NAVIGATE"
  | "REQUEST_CURRENT_RESOURCE"
  | "CHECK_AUTH"
  | "RENEW_SESSION"
  | "LOGOUT";

export type LifeAuthStatusPayload =
  | { authenticated: false }
  | { authenticated: true; user: { displayName: string } };

export type LifeActionPayload = {
  action: string;
  message: string;
  resource?: WorkspaceResource;
  traceId?: string;
};

type InboundV2Payload<T> = Omit<LifeBridgeEnvelope<T>, "payload"> & {
  payload: T;
};
type InboundV3Payload<T> = Omit<LifeBridgeEnvelope<T, 3>, "payload"> & {
  payload: T;
};

export type InboundLifeBridgeMessage =
  | (LifeBridgeEnvelope<undefined> & { type: "LIFE_READY" })
  | (InboundV2Payload<{ title: string }> & { type: "TITLE_CHANGED" })
  | (InboundV2Payload<{ url: string }> & { type: "ROUTE_CHANGED" })
  | (InboundV2Payload<{ resource: WorkspaceResource }> & {
      type: "RESOURCE_CHANGED";
    })
  | (InboundV2Payload<LifeActionPayload> & {
      type: "ACTION_COMPLETED" | "ACTION_FAILED";
    })
  | (InboundV2Payload<{
      resource?: WorkspaceResource;
      traceId?: string;
    }> & { type: "DATA_CHANGED" })
  | (InboundV2Payload<{ dirty: boolean }> & { type: "DIRTY_STATE_CHANGED" })
  | (InboundV3Payload<LifeAuthStatusPayload> & { type: "AUTH_STATUS" })
  | (InboundV3Payload<{ reason?: string }> & {
      type: "AUTH_REQUIRED" | "SESSION_EXPIRED";
    });

const MAX_REQUEST_ID = 128;
const MAX_NONCE = 256;
const MAX_MESSAGE = 500;
const ALLOWED_ACTIONS = new Set([
  "goal_created",
  "project_created",
  "action_created",
  "action_updated",
  "action_status_updated",
  "children_reordered",
  "focus_updated",
  "journal_entry_created",
  "review_created",
  "weekly_review_applied",
  "knowledge_item_created",
  "ai_execution_started",
  "ai_execution_output_appended",
  "ai_execution_finished",
  "confirmed_write_executed",
]);
const RESOURCE_TYPES = new Set([
  "dashboard",
  "domain",
  "goal",
  "project",
  "action",
  "calendar",
  "journal",
  "knowledge",
  "review",
  "ai_execution",
  "draft",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasOnlyKeys(value: Record<string, unknown>, keys: string[]): boolean {
  const allowed = new Set(keys);
  return Object.keys(value).every((key) => allowed.has(key));
}

function isBoundedText(value: unknown, max = MAX_MESSAGE): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= max;
}

function parseResource(value: unknown): WorkspaceResource | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "version",
      "extensionId",
      "type",
      "id",
      "path",
      "title",
    ]) ||
    value.version !== 1 ||
    (value.extensionId !== undefined && value.extensionId !== "life") ||
    !isBoundedText(value.type, 32) ||
    !RESOURCE_TYPES.has(value.type) ||
    !isBoundedText(value.path, 256) ||
    (value.title !== undefined && !isBoundedText(value.title, 180))
  ) {
    return null;
  }
  const uriType = value.type === "ai_execution" ? "ai-execution" : value.type;
  const uri =
    value.id === undefined
      ? `life://${uriType}`
      : typeof value.id === "string"
        ? `life://${uriType}/${encodeURIComponent(value.id)}`
        : "";
  const resolved = uri ? resolveLifeResource(uri) : null;
  if (!resolved || resolved.path !== value.path) return null;
  return {
    ...resolved,
    ...(typeof value.title === "string" ? { title: value.title } : {}),
  };
}

function parseActionPayload(value: unknown): LifeActionPayload | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, ["action", "message", "resource", "traceId"]) ||
    !isBoundedText(value.action, 80) ||
    !ALLOWED_ACTIONS.has(value.action) ||
    !isBoundedText(value.message)
  ) {
    return null;
  }
  const resource =
    value.resource === undefined ? undefined : parseResource(value.resource);
  if (value.resource !== undefined && !resource) return null;
  if (value.traceId !== undefined && !isBoundedText(value.traceId, 128))
    return null;
  return {
    action: value.action,
    message: value.message,
    ...(resource ? { resource } : {}),
    ...(typeof value.traceId === "string" ? { traceId: value.traceId } : {}),
  };
}

function parseAuthPayload(
  type: string,
  value: unknown,
): LifeAuthStatusPayload | { reason?: string } | null {
  if (!isRecord(value)) return null;
  if (type === "AUTH_STATUS") {
    if (value.authenticated === false && hasOnlyKeys(value, ["authenticated"]))
      return { authenticated: false };
    if (
      value.authenticated === true &&
      hasOnlyKeys(value, ["authenticated", "user"]) &&
      isRecord(value.user) &&
      hasOnlyKeys(value.user, ["displayName"]) &&
      isBoundedText(value.user.displayName, 180)
    ) {
      return {
        authenticated: true,
        user: { displayName: value.user.displayName },
      };
    }
    return null;
  }
  if (!hasOnlyKeys(value, ["reason"])) return null;
  if (value.reason !== undefined && !isBoundedText(value.reason, 240))
    return null;
  return typeof value.reason === "string" ? { reason: value.reason } : {};
}

/** Parses an inbound Life bridge envelope after nonce validation. */
export function parseInboundLifeBridgeMessage(
  value: unknown,
  expectedNonce: string,
): InboundLifeBridgeMessage | null {
  if (
    !isRecord(value) ||
    !hasOnlyKeys(value, [
      "version",
      "type",
      "requestId",
      "sessionNonce",
      "payload",
    ]) ||
    !isBoundedText(value.type, 64) ||
    !isBoundedText(value.requestId, MAX_REQUEST_ID) ||
    !isBoundedText(value.sessionNonce, MAX_NONCE) ||
    value.sessionNonce !== expectedNonce
  ) {
    return null;
  }
  const base = {
    requestId: value.requestId,
    sessionNonce: value.sessionNonce,
  };
  if (value.version === 3) {
    if (
      value.type !== "AUTH_STATUS" &&
      value.type !== "AUTH_REQUIRED" &&
      value.type !== "SESSION_EXPIRED"
    ) {
      return null;
    }
    const payload = parseAuthPayload(value.type, value.payload);
    return payload
      ? ({
          ...base,
          version: 3,
          type: value.type,
          payload,
        } as InboundLifeBridgeMessage)
      : null;
  }
  if (value.version !== 2) return null;
  if (value.type === "LIFE_READY" && value.payload === undefined) {
    return { ...base, version: 2, type: "LIFE_READY" };
  }
  if (!isRecord(value.payload)) return null;
  if (
    value.type === "TITLE_CHANGED" &&
    hasOnlyKeys(value.payload, ["title"]) &&
    isBoundedText(value.payload.title, 180)
  ) {
    return {
      ...base,
      version: 2,
      type: "TITLE_CHANGED",
      payload: { title: value.payload.title },
    };
  }
  if (
    value.type === "ROUTE_CHANGED" &&
    hasOnlyKeys(value.payload, ["url"]) &&
    isBoundedText(value.payload.url, 2048)
  ) {
    return {
      ...base,
      version: 2,
      type: "ROUTE_CHANGED",
      payload: { url: value.payload.url },
    };
  }
  if (
    value.type === "DIRTY_STATE_CHANGED" &&
    hasOnlyKeys(value.payload, ["dirty"]) &&
    typeof value.payload.dirty === "boolean"
  ) {
    return {
      ...base,
      version: 2,
      type: "DIRTY_STATE_CHANGED",
      payload: { dirty: value.payload.dirty },
    };
  }
  if (
    value.type === "RESOURCE_CHANGED" &&
    hasOnlyKeys(value.payload, ["resource"])
  ) {
    const resource = parseResource(value.payload.resource);
    return resource
      ? {
          ...base,
          version: 2,
          type: "RESOURCE_CHANGED",
          payload: { resource },
        }
      : null;
  }
  if (value.type === "ACTION_COMPLETED" || value.type === "ACTION_FAILED") {
    const payload = parseActionPayload(value.payload);
    return payload ? { ...base, version: 2, type: value.type, payload } : null;
  }
  if (
    value.type === "DATA_CHANGED" &&
    hasOnlyKeys(value.payload, ["resource", "traceId"])
  ) {
    const resource =
      value.payload.resource === undefined
        ? undefined
        : parseResource(value.payload.resource);
    if (value.payload.resource !== undefined && !resource) return null;
    if (
      value.payload.traceId !== undefined &&
      !isBoundedText(value.payload.traceId, 128)
    ) {
      return null;
    }
    return {
      ...base,
      version: 2,
      type: "DATA_CHANGED",
      payload: {
        ...(resource ? { resource } : {}),
        ...(typeof value.payload.traceId === "string"
          ? { traceId: value.payload.traceId }
          : {}),
      },
    };
  }
  return null;
}

/** Validates the browser event boundary before parsing its Life payload. */
export function readLifeBridgeEvent(
  event: MessageEvent,
  expectedSource: Window | null,
  config: LifeDockConfig,
  expectedNonce: string,
): InboundLifeBridgeMessage | null {
  if (event.origin !== config.origin || event.source !== expectedSource)
    return null;
  const message = parseInboundLifeBridgeMessage(event.data, expectedNonce);
  if (message?.type !== "ROUTE_CHANGED") return message;
  let target: URL;
  try {
    target = new URL(message.payload.url, `${config.origin}/`);
  } catch {
    return null;
  }
  if (
    target.origin !== config.origin ||
    !target.pathname.startsWith("/embed/") ||
    target.username ||
    target.password ||
    target.hash
  ) {
    return null;
  }
  return {
    ...message,
    payload: { url: target.href },
  };
}

/** Creates a bounded Life bridge request envelope. */
export function createLifeBridgeMessage<T>(
  type: LifeHostBridgeType,
  sessionNonce: string,
  payload?: T,
  requestId = createLifeRequestId(),
): LifeBridgeEnvelope<T, 2 | 3> {
  return {
    version:
      type === "CHECK_AUTH" || type === "RENEW_SESSION" || type === "LOGOUT"
        ? 3
        : 2,
    type,
    requestId,
    sessionNonce,
    payload,
  };
}

export function createLifeRequestId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `req-${Date.now().toString(36)}`;
}

export function createLifeSessionNonce(): string {
  const bytes = new Uint8Array(24);
  if (globalThis.crypto?.getRandomValues) {
    globalThis.crypto.getRandomValues(bytes);
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
      "",
    );
  }
  return `${Date.now().toString(36)}-${createLifeRequestId()}`;
}
