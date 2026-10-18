import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/format.dart';
import 'package:postit/ui/labels.dart';
import 'package:postit/ui/widgets/auto_text.dart';
import 'package:postit/ui/widgets/label_text.dart';

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
    final l = AppLocalizations.of(context);
    final pending = ref.watch(pendingCountProvider);
    if (!_statusChosen) {
      if (pending.isLoading && !pending.hasValue) return const Center(child: CircularProgressIndicator());
      _status = (pending.value ?? 0) > 0 ? StatusDto.pending : null;
      _statusChosen = true;
    }
    final list = ref.watch(usersListProvider(_filter));
    final chips = <(String, StatusDto?)>[
      (l.users_filter_all, null),
      (statusLabel(context, StatusDto.pending), StatusDto.pending),
      (statusLabel(context, StatusDto.active), StatusDto.active),
      (statusLabel(context, StatusDto.disabled), StatusDto.disabled),
    ];
    return Scaffold(
      appBar: AppBar(title: Text(l.users_title)),
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsetsDirectional.fromSTEB(16, 8, 16, 0),
            child: TextField(
              decoration: InputDecoration(prefixIcon: const Icon(Icons.search), hintText: l.users_search_hint),
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
          if (list.value case final page?)
            Padding(
              padding: const EdgeInsetsDirectional.fromSTEB(16, 0, 16, 8),
              child: Align(
                alignment: AlignmentDirectional.centerStart,
                child: Text(
                  l.users_count(page.total, formatNumber(context, page.total)),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
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
                  children: [
                    Padding(
                      padding: const EdgeInsets.all(32),
                      child: Center(child: Text(l.users_none)),
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
                                Text(l.users_load_more_failed),
                                TextButton(
                                  onPressed: () => ref.read(usersListProvider(_filter).notifier).loadMore(retry: true),
                                  child: Text(l.action_retry),
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
                        title: AutoDirectionText(u.displayName),
                        subtitle: AutoDirectionText(u.email ?? ''),
                        trailing: LabelText(statusLabel(context, u.status)),
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
