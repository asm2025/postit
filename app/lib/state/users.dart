import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

typedef UsersFilter = ({StatusDto? status, String search});

/// The generator names the inline `data` item type of `PageUserDto` `Data3`; it is a `UserDto`.
UserDto _user(Data3 d) => UserDto.fromJson(d.toJson());

class UsersPage {
  const UsersPage(this.items, this.total, this.page);
  final List<UserDto> items;
  final int total;
  final int page;

  bool get hasMore => items.length < total;
}

class UsersListNotifier extends AsyncNotifier<UsersPage> {
  UsersListNotifier(this.filter);

  final UsersFilter filter;
  static const pageSize = 25;
  bool _loadingMore = false;

  @override
  Future<UsersPage> build() async {
    final page = await ref
        .watch(usersRepositoryProvider)
        .list(status: filter.status, search: filter.search, page: 1, pageSize: pageSize);
    return UsersPage(page.data.map(_user).toList(), page.total, 1);
  }

  /// Appends the next page; a no-op while loading or at the end.
  Future<void> loadMore() async {
    final current = state.value;
    if (current == null || !current.hasMore || _loadingMore || state.isLoading) return;
    _loadingMore = true;
    try {
      final next = await ref
          .read(usersRepositoryProvider)
          .list(status: filter.status, search: filter.search, page: current.page + 1, pageSize: pageSize);
      state = AsyncData(UsersPage([...current.items, ...next.data.map(_user)], next.total, current.page + 1));
    } on Object catch (e, st) {
      state = AsyncError(e, st);
    } finally {
      _loadingMore = false;
    }
  }
}

final usersListProvider = AsyncNotifierProvider.autoDispose.family<UsersListNotifier, UsersPage, UsersFilter>(
  UsersListNotifier.new,
);
