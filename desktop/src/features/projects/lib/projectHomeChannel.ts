import { useProjectsQuery } from "@/features/projects/hooks";
import type { Project } from "@/features/projects/projectModels";

/** Resolves the canonical visible project home for a channel. */
export function findProjectHomeByChannelId(
  channelId: string | null | undefined,
  projects: readonly Project[],
): Project | null {
  if (!channelId) return null;
  const matching = projects
    .filter(
      (project) => !project.legacy && project.projectChannelId === channelId,
    )
    .sort((left, right) => left.createdAt - right.createdAt);
  return (
    matching.find((project) => project.visibility !== "unlisted") ??
    matching[0] ??
    null
  );
}

export function isProjectHomeChannel(
  channelId: string | null | undefined,
  projects: ReadonlyArray<{ projectChannelId: string | null }>,
): boolean {
  if (!channelId) return false;
  return projects.some((project) => project.projectChannelId === channelId);
}

export function useIsProjectHomeChannel(channelId: string | null | undefined) {
  const projectsQuery = useProjectsQuery();
  return isProjectHomeChannel(channelId, projectsQuery.data ?? []);
}
