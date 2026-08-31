import type {
  WorkspaceDockExtension,
  WorkspaceDockExtensionId,
} from "@/features/workspace-dock/workspaceDockTypes";

export type WorkspaceDockRegistry = {
  extensions: readonly WorkspaceDockExtension[];
  byId: ReadonlyMap<WorkspaceDockExtensionId, WorkspaceDockExtension>;
};

function isExactHttpOrigin(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      (url.protocol === "http:" || url.protocol === "https:") &&
      !url.username &&
      !url.password &&
      url.pathname === "/" &&
      !url.search &&
      !url.hash &&
      url.origin === value
    );
  } catch {
    return false;
  }
}

export function createWorkspaceDockRegistry(
  extensions: WorkspaceDockExtension[],
): WorkspaceDockRegistry {
  const ids = new Set<string>();
  const schemes = new Set<string>();
  for (const extension of extensions) {
    if (ids.has(extension.id))
      throw new Error(`Duplicate workspace dock id: ${extension.id}`);
    if (schemes.has(extension.scheme))
      throw new Error(`Duplicate workspace dock scheme: ${extension.scheme}`);
    ids.add(extension.id);
    schemes.add(extension.scheme);

    if (extension.origin === null || extension.homeUrl === null) {
      if (extension.origin !== null || extension.homeUrl !== null)
        throw new Error(
          `Workspace dock ${extension.id} must configure both origin and home URL`,
        );
      continue;
    }
    if (!isExactHttpOrigin(extension.origin))
      throw new Error(`Workspace dock ${extension.id} origin is invalid`);
    const home = new URL(extension.homeUrl);
    if (home.origin !== extension.origin)
      throw new Error(
        `Workspace dock ${extension.id} home URL is cross-origin`,
      );
  }
  const ordered = [...extensions].sort((left, right) =>
    left.id.localeCompare(right.id),
  );
  return {
    extensions: ordered,
    byId: new Map(ordered.map((extension) => [extension.id, extension])),
  };
}
