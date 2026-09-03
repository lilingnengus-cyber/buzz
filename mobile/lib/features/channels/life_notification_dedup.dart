import '../../shared/relay/relay.dart';

final _businessIdempotency = RegExp(r'^sha256:[0-9a-f]{64}$');

/// Returns the signer-scoped business identity of a Life notification.
///
/// NIP-17 deliberately randomizes the outer event, so relay event IDs cannot
/// deduplicate a retry. Invalid or ambiguous tags are treated as ordinary
/// messages. Scoping to the signer prevents another author from suppressing a
/// legitimate notification by copying its tags.
String? lifeNotificationDedupKey(NostrEvent event) {
  final isLifeNotification = event.tags.any(
    (tag) => tag.length == 2 && tag[0] == 'source' && tag[1] == 'life-notifier',
  );
  if (!isLifeNotification) return null;
  final keys = event.tags
      .where((tag) => tag.length == 2 && tag[0] == 'idempotency')
      .toList();
  if (keys.length != 1 || !_businessIdempotency.hasMatch(keys.single[1])) {
    return null;
  }
  return '${event.pubkey.toLowerCase()}:${keys.single[1]}';
}

/// Keeps the first chronological delivery for each Life business identity.
List<NostrEvent> dedupeLifeNotifications(Iterable<NostrEvent> events) {
  final seen = <String>{};
  final result = <NostrEvent>[];
  for (final event in events) {
    final key = lifeNotificationDedupKey(event);
    if (key == null || seen.add(key)) result.add(event);
  }
  return result;
}
