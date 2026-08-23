import type { ProjectPullRequest, Repository } from "@/features/projects/hooks";
import {
  projectRepoUnavailablePresentation,
  projectRepoUnavailableReason,
} from "@/features/projects/lib/projectRepoAvailability";
import type { ViewerGitIdentity } from "@/features/projects/lib/projectContributorMatching";
import type { ProjectRepositorySnapshotResult } from "@/features/projects/useProjectRepositorySnapshots";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import type { ProjectRepoCommit } from "@/shared/api/types";
import { BuzzLoadingState } from "@/shared/ui/BuzzLoadingState";
import { ProjectPanelState } from "./ProjectPanelState";
import { ActivityPanel } from "./ProjectDetailFeedPanels";

export function ProjectHomeCommitsPanel({
  onSelectCommit,
  profiles,
  projectId,
  pullRequests,
  results,
  viewerGitIdentity,
}: {
  onSelectCommit: (commit: ProjectRepoCommit, repository: Repository) => void;
  profiles?: UserProfileLookup;
  projectId: string;
  pullRequests: ProjectPullRequest[];
  results: ProjectRepositorySnapshotResult[];
  viewerGitIdentity?: ViewerGitIdentity | null;
}) {
  const loaded = results.filter(
    (result) => (result.snapshot?.commits.length ?? 0) > 0,
  );
  const commitItems = loaded
    .flatMap(({ repository, snapshot }) =>
      (snapshot?.commits ?? []).map((commit) => ({
        branch: repository.defaultBranch,
        commit,
        project: repository,
        projectId,
        pullRequests,
        repoContributors: snapshot?.contributors ?? [],
      })),
    )
    .sort((left, right) => right.commit.timestamp - left.commit.timestamp);
  const failed = results.filter((result) => result.error);
  const firstFailure = failed[0];
  const failure = firstFailure
    ? projectRepoUnavailablePresentation(
        projectRepoUnavailableReason(firstFailure.error),
      )
    : null;
  if (results.some((result) => result.isLoading) && loaded.length === 0) {
    return <BuzzLoadingState label="Loading commits" />;
  }
  if (loaded.length === 0) {
    return (
      <ProjectPanelState
        description={
          failure
            ? `${firstFailure.repository.name}: ${failure.description}${
                failed.length > 1
                  ? ` ${failed.length - 1} other repositories also failed.`
                  : ""
              }`
            : "Commits pushed to this project's repositories will appear here."
        }
        error={failed.length > 0}
        title={failure?.title ?? "No commits yet"}
      />
    );
  }

  const firstItem = commitItems[0];
  if (!firstItem) return null;
  return (
    <ActivityPanel
      commitItems={commitItems}
      error={null}
      isLoading={false}
      onSelectCommit={onSelectCommit}
      profiles={profiles}
      project={firstItem.project}
      projectId={projectId}
      pullRequests={pullRequests}
      repoContributors={firstItem.repoContributors}
      snapshot={null}
      viewerGitIdentity={viewerGitIdentity}
    />
  );
}
