import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useCommunities } from "@/features/communities/useCommunities";
import {
  useProjectIssuesQuery,
  useProjectPullRequestsQuery,
  useProjectRepoSnapshotQuery,
  useRepoStateQuery,
  type Project,
} from "@/features/projects/hooks";
import { useCreateProjectIssueMutation } from "@/features/projects/issueMutations";
import { gitContributorPubkeysFromCommits } from "@/features/projects/lib/projectContributorMatching";
import { resolveProjectDefaultBranch } from "@/features/projects/lib/projectBranches";
import type { ProjectHomeWorkspaceSheetTab } from "@/features/projects/lib/projectHomeWorkspaceSheet";
import { useProjectCommitDiffQuery } from "@/features/projects/useProjectCommitDiff";
import {
  CreateIssueDialog,
  type CreateIssueDialogInput,
} from "./CreateIssueDialog";
import { CreatePullRequestDialog } from "./CreatePullRequestDialog";
import { ProjectCommitDetailPanel } from "./ProjectCommitDetailPanel";
import { ActivityPanel, ContributorsPanel } from "./ProjectDetailFeedPanels";
import { ProjectHomeCodebasePanel } from "./ProjectHomeCodebasePanel";
import { ProjectIssuesPanel } from "./ProjectIssuesPanel";
import { PullRequestsPanel } from "./ProjectPullRequestsPanel";
import { PROJECT_DETAIL_PANEL_CLASS } from "./projectPanelStyles";
import { useProjectDetailPeople } from "./useProjectDetailPeople";

export type ProjectHomeWorkspaceCreateAction = {
  disabled?: boolean;
  label: string;
  onClick: () => void;
  title?: string;
};

export type ProjectHomeWorkspaceDetail = {
  backLabel: string;
  navigation: {
    commitHash?: string;
    filePath?: string;
    issueId?: string;
    pullRequestId?: string;
  };
  onBack: () => void;
};

export function ProjectHomeWorkspaceSheet({
  identityPubkey,
  onCreateActionChange,
  onDetailChange,
  onOpenCommit,
  onRepositoryAdded,
  onSelectRepository,
  project,
  projects,
  repository,
  tab,
}: {
  identityPubkey?: string;
  onCreateActionChange?: (
    action: ProjectHomeWorkspaceCreateAction | null,
  ) => void;
  onDetailChange?: (detail: ProjectHomeWorkspaceDetail | null) => void;
  onOpenCommit: (commitHash: string) => void;
  onRepositoryAdded: (repositoryId: string) => void;
  onSelectRepository: (repositoryId: string) => void;
  project: Project;
  projects: Project[];
  repository: Project["repositories"][number];
  tab: ProjectHomeWorkspaceSheetTab;
}) {
  const { goProject } = useAppNavigation();
  const { activeCommunity } = useCommunities();
  const [selectedIssueId, setSelectedIssueId] = React.useState<string | null>(
    null,
  );
  const [selectedPullRequestId, setSelectedPullRequestId] = React.useState<
    string | null
  >(null);
  const [selectedCommitHash, setSelectedCommitHash] = React.useState<
    string | null
  >(null);
  const [filesContext, setFilesContext] = React.useState<{
    kind: "file" | "folder";
    onBack?: () => void;
    path: string;
  } | null>(null);
  const [createIssueOpen, setCreateIssueOpen] = React.useState(false);
  const [createPullRequestOpen, setCreatePullRequestOpen] =
    React.useState(false);

  const issuesQuery = useProjectIssuesQuery(repository);
  const pullRequestsQuery = useProjectPullRequestsQuery(repository);
  const issues = issuesQuery.data ?? [];
  const pullRequests = pullRequestsQuery.data ?? [];
  const people = useProjectDetailPeople({
    issues,
    pullRequests,
    repository,
  });
  const repoStateQuery = useRepoStateQuery(repository);
  const defaultBranch = resolveProjectDefaultBranch(
    repository.defaultBranch,
    repoStateQuery.data,
  );
  const snapshotQuery = useProjectRepoSnapshotQuery(
    repository,
    defaultBranch,
    null,
    null,
    true,
  );
  const snapshot = snapshotQuery.data ?? null;
  const commitDiffQuery = useProjectCommitDiffQuery(
    repository,
    selectedCommitHash,
    "remote",
    activeCommunity?.reposDir,
  );
  const createIssueMutation = useCreateProjectIssueMutation(repository);
  const contributorPubkeysByGitIdentity = React.useMemo(
    () =>
      gitContributorPubkeysFromCommits(snapshot?.commits ?? [], pullRequests),
    [pullRequests, snapshot?.commits],
  );
  const selectedPullRequest =
    pullRequests.find(
      (pullRequest) => pullRequest.id === selectedPullRequestId,
    ) ?? null;
  const selectedCommit =
    snapshot?.commits.find((commit) => commit.hash === selectedCommitHash) ??
    null;
  const selectedCommitPullRequest = selectedCommitHash
    ? pullRequests.find(
        (pullRequest) =>
          pullRequest.commit === selectedCommitHash ||
          pullRequest.initialCommit === selectedCommitHash,
      )
    : null;
  const handleCreateIssue = React.useCallback(
    async (input: CreateIssueDialogInput) => {
      const issueId = await createIssueMutation.mutateAsync(input);
      toast.success("Task created.");
      await issuesQuery.refetch();
      setSelectedIssueId(issueId);
    },
    [createIssueMutation, issuesQuery],
  );
  const handlePullRequestCreated = React.useCallback(
    async (
      createdProject: Project,
      createdRepository: Project["repositories"][number],
      pullRequestId: string,
    ) => {
      if (createdProject.id !== project.id) {
        await goProject(createdProject.id, {
          pullRequestId,
          repositoryId: createdRepository.id,
        });
        return;
      }
      if (createdRepository.id !== repository.id) {
        onSelectRepository(createdRepository.id);
      }
      await pullRequestsQuery.refetch();
      setSelectedPullRequestId(pullRequestId);
    },
    [
      goProject,
      onSelectRepository,
      project.id,
      pullRequestsQuery,
      repository.id,
    ],
  );
  const detail = React.useMemo<ProjectHomeWorkspaceDetail | null>(() => {
    if (tab === "issues" && selectedIssueId) {
      return {
        backLabel: "Back to Tasks",
        navigation: { issueId: selectedIssueId },
        onBack: () => setSelectedIssueId(null),
      };
    }
    if (tab === "prs" && selectedPullRequestId) {
      return {
        backLabel: "Back to Reviews",
        navigation: { pullRequestId: selectedPullRequestId },
        onBack: () => setSelectedPullRequestId(null),
      };
    }
    if (tab === "commits" && selectedCommitHash) {
      return {
        backLabel: "Back to Commits",
        navigation: { commitHash: selectedCommitHash },
        onBack: () => setSelectedCommitHash(null),
      };
    }
    if (tab === "files" && filesContext?.onBack) {
      return {
        backLabel: "Back to Files",
        navigation: { filePath: filesContext.path },
        onBack: filesContext.onBack,
      };
    }
    return null;
  }, [
    filesContext,
    selectedCommitHash,
    selectedIssueId,
    selectedPullRequestId,
    tab,
  ]);
  React.useEffect(() => {
    onDetailChange?.(detail);
  }, [detail, onDetailChange]);
  React.useEffect(
    () => () => {
      onDetailChange?.(null);
    },
    [onDetailChange],
  );
  React.useEffect(() => {
    if (tab === "issues" && !selectedIssueId) {
      onCreateActionChange?.({
        disabled: createIssueMutation.isPending,
        label: "Create task",
        onClick: () => setCreateIssueOpen(true),
      });
      return;
    }
    if (tab === "prs" && !selectedPullRequestId) {
      onCreateActionChange?.({
        disabled: projects.length === 0,
        label: "Create review",
        onClick: () => setCreatePullRequestOpen(true),
        title: "Create review — choose a repository and branches to compare",
      });
      return;
    }
    onCreateActionChange?.(null);
  }, [
    createIssueMutation.isPending,
    onCreateActionChange,
    projects.length,
    selectedIssueId,
    selectedPullRequestId,
    tab,
  ]);
  React.useEffect(
    () => () => {
      onCreateActionChange?.(null);
    },
    [onCreateActionChange],
  );

  let body: React.ReactNode;
  switch (tab) {
    case "issues":
      body = (
        <ProjectIssuesPanel
          onSelectedIssueIdChange={setSelectedIssueId}
          profiles={people.profiles}
          project={repository}
          selectedIssueId={selectedIssueId}
        />
      );
      break;
    case "prs":
      body = (
        <PullRequestsPanel
          error={pullRequestsQuery.error}
          isLoading={pullRequestsQuery.isLoading}
          onSelectedPullRequestIdChange={setSelectedPullRequestId}
          profiles={people.profiles}
          project={repository}
          pullRequests={pullRequests}
          selectedPullRequest={selectedPullRequest}
        />
      );
      break;
    case "commits":
      body = selectedCommitHash ? (
        <ProjectCommitDetailPanel
          commit={selectedCommit}
          commitHash={selectedCommitHash}
          diff={commitDiffQuery.data}
          diffError={commitDiffQuery.error}
          diffLoading={commitDiffQuery.isLoading}
          originAgentName={selectedCommitPullRequest?.originAgentName}
          originChannelId={selectedCommitPullRequest?.channelId}
          project={repository}
        />
      ) : (
        <ActivityPanel
          branch={defaultBranch ?? undefined}
          error={snapshotQuery.error}
          isLoading={snapshotQuery.isPending}
          onSelectCommit={(commit) => setSelectedCommitHash(commit.hash)}
          profiles={people.profiles}
          project={repository}
          projectId={project.id}
          pullRequests={pullRequests}
          repoContributors={snapshot?.contributors ?? []}
          snapshot={snapshot}
          viewerGitIdentity={people.viewerGitIdentity}
        />
      );
      break;
    case "files":
      body = (
        <ProjectHomeCodebasePanel
          identityPubkey={identityPubkey}
          onFilesContextChange={setFilesContext}
          onOpenCommit={onOpenCommit}
          onRepositoryAdded={onRepositoryAdded}
          onSelectRepository={onSelectRepository}
          project={project}
          projects={projects}
          repository={repository}
        />
      );
      break;
    case "contributors":
      body = (
        <ContributorsPanel
          activityCounts={people.contributorActivityCounts}
          contributorPubkeys={people.contributorPubkeys}
          contributorPubkeysByGitIdentity={contributorPubkeysByGitIdentity}
          profiles={people.profiles}
          repoContributors={snapshot?.contributors ?? []}
        />
      );
      break;
  }

  const listPanel =
    (tab === "issues" && !selectedIssueId) ||
    (tab === "prs" && !selectedPullRequestId);

  return (
    <div
      className="-mx-4 [&_[data-project-detail-panel]]:rounded-none [&_[data-project-detail-panel]]:border-0"
      data-tab={tab}
      data-testid="project-home-workspace-sheet"
    >
      {listPanel ? (
        <div className={PROJECT_DETAIL_PANEL_CLASS} data-project-detail-panel>
          {body}
        </div>
      ) : (
        body
      )}
      {createPullRequestOpen ? (
        <CreatePullRequestDialog
          initialProjectId={project.id}
          onCreated={handlePullRequestCreated}
          onOpenChange={setCreatePullRequestOpen}
          open
          projects={projects}
          reposDir={activeCommunity?.reposDir}
        />
      ) : null}
      <CreateIssueDialog
        isCreating={createIssueMutation.isPending}
        onCreate={handleCreateIssue}
        onOpenChange={setCreateIssueOpen}
        open={createIssueOpen}
        projectName={repository.name}
      />
    </div>
  );
}
