import { useMutation, useQueryClient } from "@tanstack/react-query";

import {
  channelsQueryKey,
  upsertCachedChannel,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
import { useApplyTemplate } from "@/features/channel-templates/useApplyTemplate";
import { type Project, projectsQueryKey } from "@/features/projects/hooks";
import { isUnsupportedProjectKindError } from "@/features/projects/projectCreation";
import { buildProjectRelatedChannelPatchTemplate } from "@/features/projects/projectChannelCreation";
import { addRelatedChannelToProject } from "@/features/projects/projectModels";
import { publishOwnedAgentProjectAnnouncements } from "@/features/projects/projectOwnerControl";
import { publishProjectOwnerAnnouncement } from "@/shared/api/projectGit";
import { relayClient } from "@/shared/api/relayClient";
import type { Channel, ChannelVisibility } from "@/shared/api/types";
import { KIND_PROJECT_ANNOUNCEMENT } from "@/shared/constants/kinds";

export type AddProjectChannelInput = {
  description?: string;
  name: string;
  ownerControlAgentPubkey?: string;
  project: Project;
  templateId?: string;
  ttlSeconds?: number;
  visibility: ChannelVisibility;
};

export async function addProjectChannel(
  input: AddProjectChannelInput,
  deps: {
    applyAgents: (
      templateId: string | undefined,
      channelId: string,
    ) => void | Promise<void>;
    applyCanvas: (
      templateId: string | undefined,
      channelId: string,
      channelName: string,
    ) => Promise<void>;
    createChannel: ReturnType<typeof useCreateChannelMutation>["mutateAsync"];
    fetchEvents?: typeof relayClient.fetchEvents;
    publishOwnedAgentAnnouncements?: typeof publishOwnedAgentProjectAnnouncements;
    publishOwnerAnnouncement?: typeof publishProjectOwnerAnnouncement;
  },
): Promise<{ channel: Channel; project: Project }> {
  const {
    applyAgents,
    applyCanvas,
    createChannel,
    fetchEvents = relayClient.fetchEvents.bind(relayClient),
    publishOwnedAgentAnnouncements = publishOwnedAgentProjectAnnouncements,
    publishOwnerAnnouncement = publishProjectOwnerAnnouncement,
  } = deps;
  const targetOwner = input.project.owner.toLowerCase();
  const channel = await createChannel({
    channelType: "stream",
    description: input.description,
    name: input.name,
    ttlSeconds: input.ttlSeconds,
    visibility: input.visibility,
  });

  const liveHeads = await fetchEvents({
    authors: [targetOwner],
    kinds: [KIND_PROJECT_ANNOUNCEMENT],
    "#d": [input.project.dtag],
    limit: 1,
  });
  const liveHead = liveHeads[0];
  if (!liveHead) {
    throw new Error(
      "Could not find this project on the relay. Refresh and try again.",
    );
  }
  if (liveHead.created_at > input.project.createdAt) {
    throw new Error(
      "This project was updated by another session while you were working. Refresh and try again.",
    );
  }

  const templates = buildProjectRelatedChannelPatchTemplate({
    channelId: channel.id,
    liveHead,
    ownerPubkey: targetOwner,
  });
  let projectCreatedAt = liveHead.created_at;
  if (!templates.alreadyBound) {
    const createdAt = Math.max(
      Math.floor(Date.now() / 1_000),
      liveHead.created_at + 1,
    );
    try {
      if (input.ownerControlAgentPubkey) {
        const events = await publishOwnedAgentAnnouncements(
          input.ownerControlAgentPubkey,
          [{ ...templates.project, createdAt }],
        );
        projectCreatedAt =
          events.find((event) => event.kind === KIND_PROJECT_ANNOUNCEMENT)
            ?.created_at ?? createdAt;
      } else {
        const publication = await publishOwnerAnnouncement({
          ...templates.project,
          createdAt,
          targetOwner,
        });
        if (publication.publicationError) {
          throw new Error(publication.publicationError);
        }
        projectCreatedAt = publication.event.created_at;
      }
    } catch (error) {
      if (isUnsupportedProjectKindError(error)) {
        throw new Error(
          "This relay does not support projects yet, so the channel could not be linked.",
        );
      }
      throw error;
    }
  }

  await applyCanvas(input.templateId, channel.id, input.name);
  void applyAgents(input.templateId, channel.id);

  return {
    channel,
    project: addRelatedChannelToProject(
      input.project,
      channel.id,
      projectCreatedAt,
    ),
  };
}

export function useAddProjectChannelMutation() {
  const queryClient = useQueryClient();
  const createChannelMutation = useCreateChannelMutation();
  const { applyAgents, applyCanvas } = useApplyTemplate();

  return useMutation({
    mutationFn: (input: AddProjectChannelInput) =>
      addProjectChannel(input, {
        applyAgents,
        applyCanvas,
        createChannel: createChannelMutation.mutateAsync,
      }),
    onSuccess: ({ channel, project }) => {
      queryClient.setQueryData<Project[]>(projectsQueryKey, (current = []) =>
        current.map((candidate) =>
          candidate.id === project.id ? project : candidate,
        ),
      );
      queryClient.setQueryData<Channel[]>(channelsQueryKey, (current) =>
        upsertCachedChannel(current, channel),
      );
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: channelsQueryKey,
        refetchType: "none",
      });
    },
  });
}
