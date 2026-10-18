import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';
import 'package:postit/ui/widgets/theme_control.dart';
import 'package:url_launcher/url_launcher.dart';

/// Disabled while signing out, so a second tap cannot start a second sign-out.
class _SignOutButton extends ConsumerStatefulWidget {
  const _SignOutButton();

  @override
  ConsumerState<_SignOutButton> createState() => _SignOutButtonState();
}

class _SignOutButtonState extends ConsumerState<_SignOutButton> {
  bool _busy = false;

  Future<void> _signOut() async {
    final manager = ref.read(sessionManagerProvider).value;
    if (manager == null) return;
    setState(() => _busy = true);
    try {
      await manager.signOut();
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) =>
      OutlinedButton(onPressed: _busy ? null : _signOut, child: const Text('Sign out'));
}

class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final me = ref.watch(meProvider).value;
    if (me == null) return const SizedBox.shrink();
    final text = Theme.of(context).textTheme;
    final accountUrl = me.accountUrl;
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text('Profile', style: text.headlineMedium),
            const SizedBox(height: 16),
            AccountCard(me: me),
            const SizedBox(height: 8),
            Text('Your name and email come from your sign-in provider.', style: text.bodySmall),
            if (accountUrl != null)
              TextButton.icon(
                onPressed: () => launchUrl(Uri.parse(accountUrl), mode: LaunchMode.externalApplication),
                icon: const Icon(Icons.open_in_new),
                label: const Text('Manage your account'),
              ),
            const SizedBox(height: 24),
            Text('THEME', style: text.labelSmall),
            const SizedBox(height: 8),
            const ThemeControl(),
            const SizedBox(height: 24),
            const _SignOutButton(),
            const SizedBox(height: 32),
            Text('DANGER ZONE', style: text.labelSmall?.copyWith(color: context.tokens.danger)),
            const SizedBox(height: 8),
            OutlinedButton(
              style: OutlinedButton.styleFrom(foregroundColor: context.tokens.danger),
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => _DeleteAccountDialog(displayName: me.displayName),
              ),
              child: const Text('Delete my postit account'),
            ),
          ],
        ),
      ),
    );
  }
}

class _DeleteAccountDialog extends ConsumerStatefulWidget {
  const _DeleteAccountDialog({required this.displayName});
  final String displayName;

  @override
  ConsumerState<_DeleteAccountDialog> createState() => _DeleteAccountDialogState();
}

class _DeleteAccountDialogState extends ConsumerState<_DeleteAccountDialog> {
  final _controller = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _delete() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref.read(meRepositoryProvider).delete(_controller.text);
      if (mounted) Navigator.of(context).pop();
      await ref.read(sessionManagerProvider).value?.signOut();
    } on Object catch (e) {
      if (mounted) setState(() => _error = messageFor(context, e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final matches = _controller.text == widget.displayName;
    return AlertDialog(
      title: const Text('Delete your postit account?'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('This removes your postit account and cannot be undone. Type "${widget.displayName}" to confirm.'),
          const SizedBox(height: 12),
          TextField(controller: _controller, autofocus: true, onChanged: (_) => setState(() {})),
          if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!)),
        ],
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: context.tokens.danger),
          onPressed: matches && !_busy ? _delete : null,
          child: const Text('Delete account'),
        ),
      ],
    );
  }
}
