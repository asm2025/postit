import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';

String statusLabel(StatusDto s) => switch (s) {
  StatusDto.pending => 'Pending',
  StatusDto.active => 'Active',
  StatusDto.disabled => 'Disabled',
  _ => 'Deleting',
};

class UsersScreen extends ConsumerStatefulWidget {
  const UsersScreen({super.key});

  @override
  ConsumerState<UsersScreen> createState() => _UsersScreenState();
}

class _UsersScreenState extends ConsumerState<UsersScreen> {
  final _scroll = ScrollController();
  StatusDto? _status;
  bool _statusChosen = false;
  String _search = '';

  UsersFilter get _filter => (status: _status, search: _search);

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() {
      if (_scroll.position.extentAfter < 300) ref.read(usersListProvider(_filter).notifier).loadMore();
    });
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  /// When a page does not fill the screen (a tablet, the rail layout) nothing scrolls, so the
  /// next page is requested after layout instead.
  Widget _fillViewport(UsersPage value, Widget list) {
    if (value.hasMore) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _scroll.hasClients && _scroll.position.maxScrollExtent <= 0) {
          ref.read(usersListProvider(_filter).notifier).loadMore();
        }
      });
    }
    return list;
  }

  @override
  Widget build(BuildContext context) {
    final pending = ref.watch(pendingCountProvider);
    if (!_statusChosen) {
      if (pending.isLoading && !pending.hasValue) return const Center(child: CircularProgressIndicator());
      _status = (pending.value ?? 0) > 0 ? StatusDto.pending : null;
      _statusChosen = true;
    }
    final list = ref.watch(usersListProvider(_filter));
    final chips = <(String, StatusDto?)>[
      ('All', null),
      ('Pending', StatusDto.pending),
      ('Active', StatusDto.active),
      ('Disabled', StatusDto.disabled),
    ];
    return Scaffold(
      appBar: AppBar(title: const Text('Users')),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
            child: TextField(
              decoration: const InputDecoration(prefixIcon: Icon(Icons.search), hintText: 'Search by name or email'),
              textInputAction: TextInputAction.search,
              onSubmitted: (v) => setState(() => _search = v.trim()),
            ),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
            child: Wrap(
              spacing: 8,
              children: [
                for (final (label, status) in chips)
                  ChoiceChip(
                    label: Text(label),
                    selected: _status == status,
                    onSelected: (_) => setState(() => _status = status),
                  ),
              ],
            ),
          ),
          Expanded(
            child: RefreshIndicator(
              onRefresh: () async {
                ref
                  ..invalidate(usersListProvider(_filter))
                  ..invalidate(pendingCountProvider);
              },
              child: switch (list) {
                AsyncData(:final value) when value.items.isEmpty => ListView(
                  children: const [
                    Padding(
                      padding: EdgeInsets.all(32),
                      child: Center(child: Text('No users match.')),
                    ),
                  ],
                ),
                AsyncData(:final value) => _fillViewport(
                  value,
                  ListView.builder(
                    controller: _scroll,
                    physics: const AlwaysScrollableScrollPhysics(),
                    itemCount: value.items.length + (value.hasMore ? 1 : 0),
                    itemBuilder: (context, i) {
                      if (i >= value.items.length) {
                        if (value.moreError != null) {
                          return Padding(
                            padding: const EdgeInsets.all(16),
                            child: Column(
                              children: [
                                const Text("Couldn't load more."),
                                TextButton(
                                  onPressed: () => ref.read(usersListProvider(_filter).notifier).loadMore(retry: true),
                                  child: const Text('Retry'),
                                ),
                              ],
                            ),
                          );
                        }
                        return const Padding(
                          padding: EdgeInsets.all(16),
                          child: Center(child: CircularProgressIndicator()),
                        );
                      }
                      final u = value.items[i];
                      return ListTile(
                        title: Text(u.displayName),
                        subtitle: Text(u.email ?? ''),
                        trailing: Text(
                          statusLabel(u.status).toUpperCase(),
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                        onTap: () => context.go('/users/${u.id}'),
                      );
                    },
                  ),
                ),
                AsyncError(:final error) => ListView(
                  children: [
                    Padding(
                      padding: const EdgeInsets.all(32),
                      child: Center(child: Text(messageFor(context, error))),
                    ),
                  ],
                ),
                _ => const Center(child: CircularProgressIndicator()),
              },
            ),
          ),
        ],
      ),
    );
  }
}
