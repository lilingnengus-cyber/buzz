import type { Channel, ChannelMember } from "@/shared/api/types";
import { normalizePubkey } from "@/shared/lib/pubkey";

/**
 * Return the semantic recipients for an outgoing message.
 *
 * Stream messages notify only explicit mentions. A DM addresses every other
 * participant, so it must carry recipient `p` tags even when the composer text
 * contains no `@mention`. Agent harnesses and human notification subscriptions
 * both rely on those tags.
 */
export function messageMentionPubkeys(
  channel: Channel,
  senderPubkey: string,
  explicitMentions: readonly string[] = [],
): string[] {
  const candidates =
    channel.channelType === "dm"
      ? [
          ...explicitMentions,
          ...channel.memberPubkeys,
          ...channel.participantPubkeys,
        ]
      : explicitMentions;
  const sender = normalizePubkey(senderPubkey);

  return [...new Set(candidates.map(normalizePubkey))].filter(
    (pubkey) => pubkey.length > 0 && pubkey !== sender,
  );
}

/** Resolve incomplete DM metadata before signing a message for delivery. */
export async function resolveMessageMentionPubkeys(
  channel: Channel,
  senderPubkey: string,
  explicitMentions: readonly string[] | undefined,
  loadMembers: (channelId: string) => Promise<ChannelMember[]>,
): Promise<string[]> {
  if (channel.channelType !== "dm") {
    return messageMentionPubkeys(channel, senderPubkey, explicitMentions);
  }
  // Explicit mentions do not prove the DM roster is complete. After reconnect
  // the channel cache can exist before its participant metadata has arrived.
  const recipients = messageMentionPubkeys(channel, senderPubkey);
  if (recipients.length >= Math.max(1, channel.memberCount - 1)) {
    return messageMentionPubkeys(channel, senderPubkey, explicitMentions);
  }
  const members = await loadMembers(channel.id);
  const resolved = {
    ...channel,
    memberPubkeys: members.map((member) => member.pubkey),
    participantPubkeys: [],
  };
  if (messageMentionPubkeys(resolved, senderPubkey).length === 0) {
    throw new Error("私聊收件人暂未加载完成，请稍后重试。");
  }
  return messageMentionPubkeys(resolved, senderPubkey, explicitMentions);
}
