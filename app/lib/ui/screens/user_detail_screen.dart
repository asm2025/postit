import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/labels.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/auto_text.dart';
import 'package:postit/ui/widgets/label_text.dart';

class UserDetailScreen extends ConsumerStatefulWidget {
  const UserDetailScreen({super.key, required this.id});
  final String id;

  @override
  ConsumerState<UserDetailScreen> createState() => _UserDetailScreenState();
}

class _UserDetailScreenState extends ConsumerState<UserDetailScreen> {
  bool _busy = false;
  String? _error;

  Future<bool> _confirm(String title, String body, String action) async =>
      await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(body),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: Text(AppLocalizations.of(context).action_cancel),
            ),
            FilledButton(
              style: FilledButton.styleFrom(backgroundColor: context.tokens.danger),
              onPressed: () => Navigator.of(context).pop(true),
              child: Text(action),
            ),
          ],
        ),
      ) ??
      false;

  Future<void> _run(Future<void> Function() action, {bool leave = false}) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
      ref
        ..invalidate(usersListProvider)
        ..invalidate(pendingCountProvider)
        ..invalidate(activeAdminCountProvider);
      // Acting on yourself (say, Make member) changes /me: the gate then drops the Users tab.
      if (ref.read(meProvider).value?.id == widget.id) ref.invalidate(meProvider);
      if (leave) {
        if (mounted) context.go('/users');
      } else {
        ref.invalidate(userProvider(widget.id));
      }
    } on Object catch (e) {
      if (mounted) setState(() => _error = messageFor(context, e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final meId = ref.watch(meProvider).value?.id;
    final user = ref.watch(userProvider(widget.id));
    return Scaffold(
      appBar: AppBar(title: Text(l.user_title)),
      body: switch (user) {
        AsyncData(value: final u) => ListView(
          padding: const EdgeInsets.all(16),
          children: [
            AutoDirectionText(u.displayName, style: Theme.of(context).textTheme.headlineSmall),
            if (u.email != null) AutoDirectionText(u.email!),
            const SizedBox(height: 8),
            LabelText('${statusLabel(context, u.status)} · ${roleLabel(context, u.role)}'),
            const SizedBox(height: 24),
            if (_error != null) Padding(padding: const EdgeInsetsDirectional.only(bottom: 12), child: Text(_error!)),
            ..._actions(u, isSelf: u.id == meId),
          ],
        ),
        AsyncError(:final error) => Center(child: Text(messageFor(context, error))),
        _ => const Center(child: CircularProgressIndicator()),
      },
    );
  }

  List<Widget> _actions(UserDto u, {required bool isSelf}) {
    final l = AppLocalizations.of(context);
    final users = ref.read(usersRepositoryProvider);
    Widget button(String label, VoidCallback onPressed, {bool primary = false}) => Padding(
      padding: const EdgeInsetsDirectional.only(bottom: 12),
      child: primary
          ? FilledButton(onPressed: _busy ? null : onPressed, child: Text(label))
          : OutlinedButton(onPressed: _busy ? null : onPressed, child: Text(label)),
    );
    final roleTarget = u.role == RoleDto.admin ? RoleDto.member : RoleDto.admin;
    final roleAction = u.role == RoleDto.admin ? l.action_make_member : l.action_make_admin;
    return switch (u.status) {
      StatusDto.pending => [
        button(l.action_approve, () => _run(() => users.approve(u.id)), primary: true),
        button(l.action_reject, () async {
          if (await _confirm(l.user_reject_title(u.displayName), l.user_reject_body, l.action_reject)) {
            await _run(() => users.delete(u.id), leave: true);
          }
        }),
      ],
      StatusDto.active || StatusDto.disabled => [
        if (u.status == StatusDto.active)
          button(l.action_disable, () async {
            if (await _confirm(l.user_disable_title(u.displayName), l.user_disable_body, l.action_disable)) {
              await _run(() => users.disable(u.id));
            }
          })
        else
          button(l.action_enable, () => _run(() => users.enable(u.id)), primary: true),
        button(roleAction, () async {
          if (roleTarget == RoleDto.admin ||
              await _confirm(l.user_make_member_title(u.displayName), l.user_make_member_body, l.action_make_member)) {
            await _run(() => users.setRole(u.id, roleTarget));
          }
        }),
        if (!isSelf)
          button(l.action_delete, () async {
            if (await _confirm(l.user_delete_title(u.displayName), l.user_delete_body, l.action_delete)) {
              await _run(() => users.delete(u.id), leave: true);
            }
          }),
      ],
      _ => [Text(l.user_being_deleted)],
    };
  }
}
