import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";
import type { LifeDockConfig } from "./lifeDockConfig";

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

function hasTraversalSegment(value: string): boolean {
  const separator = value.indexOf("://");
  const pathStart = separator < 0 ? -1 : value.indexOf("/", separator + 3);
  if (pathStart < 0) return false;
  const path = value.slice(pathStart).split(/[?#]/u, 1)[0];
  return path.split("/").some((segment) => {
    try {
      const decoded = decodeURIComponent(segment);
      return decoded === "." || decoded === "..";
    } catch {
      return true;
    }
  });
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
  if (
    typeof input !== "string" ||
    input.length > 512 ||
    hasTraversalSegment(input)
  )
    return null;

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

/** Builds a same-origin LifeOS embed URL from a validated resource. */
export function buildLifeUrl(
  resource: WorkspaceResource,
  config: LifeDockConfig,
): string | null {
  if (resource.extensionId !== "life") return null;
  const reference = buildLifeReference(resource);
  const canonical = reference ? resolveLifeResource(reference) : null;
  if (!canonical || canonical.path !== resource.path) return null;
  const url = new URL(canonical.path, config.origin);
  return url.origin === config.origin ? url.href : null;
}

/** Converts a same-origin, fixed LifeOS embed URL back into a resource. */
export function parseLifeUrl(
  value: string,
  config: LifeDockConfig,
): WorkspaceResource | null {
  let url: URL;
  try {
    url = new URL(value, config.origin);
  } catch {
    return null;
  }
  if (
    url.origin !== config.origin ||
    url.username ||
    url.password ||
    url.hash
  ) {
    return null;
  }
  if (url.pathname === "/embed/dashboard" && !url.search) {
    return resolveLifeResource("life://dashboard");
  }
  if (url.pathname === "/embed/calendar") {
    if ([...url.searchParams.keys()].some((key) => key !== "date")) return null;
    const values = url.searchParams.getAll("date");
    return values.length === 1
      ? resolveLifeResource(`life://calendar/${encodeURIComponent(values[0])}`)
      : null;
  }
  if (url.search) return null;
  const match = url.pathname.match(
    /^\/embed\/(domains|goals|projects|actions|journal|knowledge|reviews|ai-executions|drafts)\/([^/]+)$/u,
  );
  if (!match) return null;
  const typeByRoute: Record<string, string> = {
    domains: "domain",
    goals: "goal",
    projects: "project",
    actions: "action",
    journal: "journal",
    knowledge: "knowledge",
    reviews: "review",
    "ai-executions": "ai-execution",
    drafts: "draft",
  };
  const type = typeByRoute[match[1]];
  return type ? resolveLifeResource(`life://${type}/${match[2]}`) : null;
}

/** Formats a safe shareable Life resource reference. */
export function buildLifeReference(resource: WorkspaceResource): string | null {
  if (resource.extensionId !== "life") return null;
  const type =
    resource.type === "ai_execution" ? "ai-execution" : resource.type;
  const value = resource.id
    ? `life://${type}/${encodeURIComponent(resource.id)}`
    : `life://${type}`;
  return resolveLifeResource(value)?.path === resource.path ? value : null;
}

export function formatLifeResourceLabel(resource: WorkspaceResource): string {
  const label = resource.type.replaceAll("_", " ");
  return resource.title ?? (resource.id ? `${label}: ${resource.id}` : label);
}
