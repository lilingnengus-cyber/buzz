import 'package:buzz/features/channels/life_notification_dedup.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';

NostrEvent notification({
  required String id,
  required String pubkey,
  required int createdAt,
  List<List<String>>? tags,
}) => NostrEvent(
  id: id,
  pubkey: pubkey,
  createdAt: createdAt,
  kind: EventKind.streamMessageV2,
  tags:
      tags ??
      [
        ['source', 'life-notifier'],
        ['idempotency', 'sha256:${'d' * 64}'],
      ],
  content: '一个项目已创建',
  sig: 'a' * 128,
);

void main() {
  test(
    'keeps the first Life notification delivery across event-id retries',
    () {
      final signer = 'a' * 64;
      final first = notification(id: '1' * 64, pubkey: signer, createdAt: 1);
      final retry = notification(id: '2' * 64, pubkey: signer, createdAt: 2);

      expect(dedupeLifeNotifications([first, retry]), [first]);
    },
  );

  test('does not deduplicate across signers or ambiguous tags', () {
    final first = notification(id: '1' * 64, pubkey: 'a' * 64, createdAt: 1);
    final otherSigner = notification(
      id: '2' * 64,
      pubkey: 'b' * 64,
      createdAt: 2,
    );
    final ambiguous = notification(
      id: '3' * 64,
      pubkey: 'a' * 64,
      createdAt: 3,
      tags: [
        ['source', 'life-notifier'],
        ['idempotency', 'sha256:${'d' * 64}'],
        ['idempotency', 'sha256:${'d' * 64}'],
      ],
    );

    expect(dedupeLifeNotifications([first, otherSigner, ambiguous]), [
      first,
      otherSigner,
      ambiguous,
    ]);
  });
}
