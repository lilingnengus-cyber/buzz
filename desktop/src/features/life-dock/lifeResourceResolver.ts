import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

const MAX_RESOURCE_ID_LENGTH = 128;
const MAX_RESOURCE_PATH_LENGTH = 256;
const RESOURCE_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._~-]{0,127}$/;
const CALENDAR_DATE_PATTERN = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;

const RESOURCE_PATHS = {
  domain: "domains",
  goal: "goals",
  project: "projects",
  action: "actions",
  journal: "journal",
  knowledge: "knowledge",
  review: "reviews",
  "ai-execution": "ai-executions",
  draft: "drafts",
} as const;

function hasUnsafeEncoding(value: string): boolean {
  return /%25/i.test(value);
}

function decodeResourceId(encoded: string): string | null {
  if (!encoded || hasUnsafeEncoding(encoded)) return null;
  try {
    const decoded = decodeURIComponent(encoded);
    if (
      decoded.length === 0 ||
      decoded.length > MAX_RESOURCE_ID_LENGTH ||
      !RESOURCE_ID_PATTERN.test(decoded) ||
      decoded === "." ||
      decoded === ".."
    ) {
      return null;
    }
    return decoded;
  } catch {
    return null;
  }
}

function isValidCalendarDate(value: string): boolean {
  if (!CALENDAR_DATE_PATTERN.test(value)) return false;
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
}

/** Resolves a strict, non-authorizing Life resource URI to an embed route. */
export function resolveLifeResource(
  input: string | object,
): WorkspaceResource | null {
  if (typeof input !== "string" || input.length > 512) return null;

  let url: URL;
  try {
    url = new URL(input);
  } catch {
    return null;
  }

  if (
    url.protocol !== "life:" ||
    url.username ||
    url.password ||
    url.port ||
    url.hash ||
    url.search ||
    !url.hostname ||
    hasUnsafeEncoding(input)
  ) {
    return null;
  }

  const segments = url.pathname.split("/").filter(Boolean);
  if (url.hostname === "dashboard") {
    if (segments.length !== 0) return null;
    return {
      version: 1,
      extensionId: "life",
      type: "dashboard",
      path: "/embed/dashboard",
    };
  }

  if (segments.length !== 1) return null;
  const id = decodeResourceId(segments[0]);
  if (!id) return null;

  if (url.hostname === "calendar") {
    if (!isValidCalendarDate(id)) return null;
    return {
      version: 1,
      extensionId: "life",
      type: "calendar",
      id,
      path: `/embed/calendar?date=${id}`,
    };
  }

  const route = RESOURCE_PATHS[url.hostname as keyof typeof RESOURCE_PATHS];
  if (!route) return null;
  const path = `/embed/${route}/${encodeURIComponent(id)}`;
  if (path.length > MAX_RESOURCE_PATH_LENGTH) return null;

  return {
    version: 1,
    extensionId: "life",
    type: url.hostname === "ai-execution" ? "ai_execution" : url.hostname,
    id,
    path,
  };
}
