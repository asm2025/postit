import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';
import 'package:postit/ui/widgets/label_text.dart';
import 'package:postit/ui/widgets/language_switch.dart';
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
      OutlinedButton(onPressed: _busy ? null : _signOut, child: Text(AppLocalizations.of(context).action_sign_out));
}

class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
    final me = ref.watch(meProvider).value;
    if (me == null) return const SizedBox.shrink();
    final text = Theme.of(context).textTheme;
    final accountUrl = me.accountUrl;
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text(l.profile_title, style: text.headlineMedium),
            const SizedBox(height: 16),
            AccountCard(me: me),
            const SizedBox(height: 8),
            Text(l.profile_idp_note, style: text.bodySmall),
            if (accountUrl != null)
              TextButton.icon(
                onPressed: () => launchUrl(Uri.parse(accountUrl), mode: LaunchMode.externalApplication),
                icon: const Icon(Icons.open_in_new),
                label: Text(l.profile_manage_account),
              ),
            const SizedBox(height: 24),
            LabelText(l.language_label),
            const SizedBox(height: 8),
            const LanguageSwitch(persist: true),
            const SizedBox(height: 24),
            LabelText(l.profile_theme),
            const SizedBox(height: 8),
            const ThemeControl(),
            const SizedBox(height: 24),
            const _SignOutButton(),
            const SizedBox(height: 32),
            LabelText(l.profile_danger_zone, color: context.tokens.danger),
            const SizedBox(height: 8),
            OutlinedButton(
              style: OutlinedButton.styleFrom(foregroundColor: context.tokens.danger),
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => _DeleteAccountDialog(displayName: me.displayName),
              ),
              child: Text(l.profile_delete_button),
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
    final l = AppLocalizations.of(context);
    final matches = _controller.text == widget.displayName;
    return AlertDialog(
      title: Text(l.profile_delete_title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l.profile_delete_body(widget.displayName)),
          const SizedBox(height: 12),
          TextField(controller: _controller, autofocus: true, onChanged: (_) => setState(() {})),
          if (_error != null) Padding(padding: const EdgeInsetsDirectional.only(top: 8), child: Text(_error!)),
        ],
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l.action_cancel)),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: context.tokens.danger),
          onPressed: matches && !_busy ? _delete : null,
          child: Text(l.profile_delete_confirm),
        ),
      ],
    );
  }
}
