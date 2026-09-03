import type { RelayEvent } from "@/shared/api/types";

export function channelMessagesKey(channelId: string) {
  return ["channel-messages", channelId] as const;
}

export function channelWindowKey(channelId: string) {
  return ["channel-window", channelId] as const;
}

export function threadRepliesKey(channelId: string, rootId: string) {
  return ["thread-replies", channelId, rootId] as const;
}

export function dedupeMessagesById(messages: RelayEvent[]) {
  const seenIds = new Set<string>();
  const deduped: RelayEvent[] = [];

  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];

    if (seenIds.has(message.id)) {
      continue;
    }

    seenIds.add(message.id);
    deduped.push(message);
  }

  return deduped.reverse();
}

const LIFE_NOTIFICATION_IDEMPOTENCY = /^sha256:[0-9a-f]{64}$/u;

export function getLifeNotificationDedupKey(message: RelayEvent) {
  const isLifeNotification = message.tags.some(
    (tag) =>
      tag.length === 2 && tag[0] === "source" && tag[1] === "life-notifier",
  );
  if (!isLifeNotification) return null;
  const idempotencyTags = message.tags.filter(
    (tag) => tag.length === 2 && tag[0] === "idempotency",
  );
  if (
    idempotencyTags.length !== 1 ||
    !LIFE_NOTIFICATION_IDEMPOTENCY.test(idempotencyTags[0][1] ?? "")
  ) {
    return null;
  }
  return `${message.pubkey.toLowerCase()}:${idempotencyTags[0][1]}`;
}

/**
 * Keeps the first accepted delivery of a Life notification. NIP-17 wraps are
 * intentionally randomized, so a retry can have a different outer event ID;
 * the signed inner business idempotency tag is the stable display identity.
 * Including the signer prevents an unrelated author from suppressing a real
 * notification by copying its public tags.
 */
export function dedupeLifeNotifications(messages: RelayEvent[]) {
  const seen = new Set<string>();
  return messages.filter((message) => {
    const key = getLifeNotificationDedupKey(message);
    if (!key) return true;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function sortMessages(messages: RelayEvent[]) {
  const sorted = dedupeMessagesById(messages).sort((left, right) => {
    if (left.created_at !== right.created_at) {
      return left.created_at - right.created_at;
    }
    // Tiebreak same-second events on id so the merge order is deterministic.
    // Without this, two events sharing a created_at can land in a different
    // position depending on which REQ (history vs live-sub) delivered them
    // first — reading as a "missing"/shuffled message at a fixed scroll offset.
    return left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  });
  return dedupeLifeNotifications(sorted);
}

export function normalizeTimelineMessages(messages: RelayEvent[]) {
  return sortMessages(messages);
}

function isOlderHistoryPage(current: RelayEvent[], history: RelayEvent[]) {
  if (current.length === 0 || history.length === 0) {
    return false;
  }

  const sortedCurrent = sortMessages(current);
  const sortedHistory = sortMessages(history);
  const newestHistory = sortedHistory[sortedHistory.length - 1]?.created_at;
  const oldestCurrent = sortedCurrent[0]?.created_at;

  if (newestHistory === undefined || oldestCurrent === undefined) {
    return false;
  }

  return newestHistory <= oldestCurrent;
}

function normalizeTimelineHistoryMessages(
  current: RelayEvent[],
  history: RelayEvent[],
) {
  return sortMessages([...current, ...history]);
}

export function mergeTimelineHistoryMessages(
  current: RelayEvent[],
  history: RelayEvent[],
) {
  if (isOlderHistoryPage(current, history)) {
    return normalizeTimelineHistoryMessages(current, history);
  }

  return normalizeTimelineMessages([...current, ...history]);
}
