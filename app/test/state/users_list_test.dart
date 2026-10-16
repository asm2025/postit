import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';

import '../helpers/fake_users.dart';

void main() {
  test('loadMore appends the next page until total is reached', () async {
    final repo = FakeUsersRepository(List.generate(30, (i) => fakeUser('u$i')));
    final container = ProviderContainer(
      overrides: [usersRepositoryProvider.overrideWithValue(repo)],
      retry: (retryCount, error) => null,
    );
    addTearDown(container.dispose);
    const UsersFilter filter = (status: null, search: '');
    final first = await container.read(usersListProvider(filter).future);
    expect(first.items, hasLength(UsersListNotifier.pageSize));
    expect(first.hasMore, isTrue);
    await container.read(usersListProvider(filter).notifier).loadMore();
    final all = container.read(usersListProvider(filter)).value!;
    expect(all.items, hasLength(30));
    expect(all.hasMore, isFalse);
    expect(repo.pagesRequested, [1, 2]);
  });
}
