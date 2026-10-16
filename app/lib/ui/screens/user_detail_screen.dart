import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/screens/users_screen.dart';
import 'package:postit/ui/theme/tokens.dart';

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
            TextButton(onPressed: () => Navigator.of(context).pop(false), child: const Text('Cancel')),
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
      if (mounted) setState(() => _error = messageFor(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final meId = ref.watch(meProvider).value?.id;
    final user = ref.watch(userProvider(widget.id));
    return Scaffold(
      appBar: AppBar(title: const Text('User')),
      body: switch (user) {
        AsyncData(value: final u) => ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text(u.displayName, style: Theme.of(context).textTheme.headlineSmall),
            if (u.email != null) Text(u.email!),
            const SizedBox(height: 8),
            Text(
              '${statusLabel(u.status).toUpperCase()} · ${u.role == RoleDto.admin ? 'ADMIN' : 'MEMBER'}',
              style: Theme.of(context).textTheme.labelSmall,
            ),
            const SizedBox(height: 24),
            if (_error != null) Padding(padding: const EdgeInsets.only(bottom: 12), child: Text(_error!)),
            ..._actions(u, isSelf: u.id == meId),
          ],
        ),
        AsyncError(:final error) => Center(child: Text(messageFor(error))),
        _ => const Center(child: CircularProgressIndicator()),
      },
    );
  }

  List<Widget> _actions(UserDto u, {required bool isSelf}) {
    final users = ref.read(usersRepositoryProvider);
    Widget button(String label, VoidCallback onPressed, {bool primary = false}) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: primary
          ? FilledButton(onPressed: _busy ? null : onPressed, child: Text(label))
          : OutlinedButton(onPressed: _busy ? null : onPressed, child: Text(label)),
    );
    final roleTarget = u.role == RoleDto.admin ? RoleDto.member : RoleDto.admin;
    final roleLabel = u.role == RoleDto.admin ? 'Make member' : 'Make admin';
    return switch (u.status) {
      StatusDto.pending => [
        button('Approve', () => _run(() => users.approve(u.id)), primary: true),
        button('Reject', () async {
          if (await _confirm('Reject ${u.displayName}?', 'Their pending account is deleted.', 'Reject')) {
            await _run(() => users.delete(u.id), leave: true);
          }
        }),
      ],
      StatusDto.active || StatusDto.disabled => [
        if (u.status == StatusDto.active)
          button('Disable', () async {
            if (await _confirm('Disable ${u.displayName}?', 'They lose access until re-enabled.', 'Disable')) {
              await _run(() => users.disable(u.id));
            }
          })
        else
          button('Enable', () => _run(() => users.enable(u.id)), primary: true),
        button(roleLabel, () async {
          if (roleTarget == RoleDto.admin ||
              await _confirm('Make ${u.displayName} a member?', 'They lose the admin screens.', 'Make member')) {
            await _run(() => users.setRole(u.id, roleTarget));
          }
        }),
        if (!isSelf)
          button('Delete', () async {
            if (await _confirm('Delete ${u.displayName}?', 'This cannot be undone.', 'Delete')) {
              await _run(() => users.delete(u.id), leave: true);
            }
          }),
      ],
      _ => [const Text('This user is being deleted.')],
    };
  }
}
