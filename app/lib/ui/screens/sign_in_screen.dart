import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_manager.dart' show sessionExpiredNotice;
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/widgets/language_switch.dart';
import 'package:postit/ui/widgets/logo.dart';

class SignInScreen extends ConsumerStatefulWidget {
  const SignInScreen({super.key});

  @override
  ConsumerState<SignInScreen> createState() => _SignInScreenState();
}

class _SignInScreenState extends ConsumerState<SignInScreen> {
  bool _busy = false;
  // Kept as data, not text, so the message follows the language on screen if it changes.
  bool _incomplete = false;
  Object? _error;

  Future<void> _signIn() async {
    final manager = ref.read(sessionManagerProvider).value;
    if (manager == null) return;
    setState(() {
      _busy = true;
      _error = null;
      _incomplete = false;
    });
    try {
      await manager.signIn();
    } on AuthCancelled {
      // The user closed the browser: nothing to report.
    } on AuthFailed {
      // The provider's own description is for logs, not people (it can be an internal code).
      _incomplete = true;
    } on Object catch (e) {
      // AuthUnavailable reads as "can't reach postit"; anything unexpected gets the generic
      // message rather than vanishing.
      _error = e;
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final notice = ref.watch(sessionStateProvider).notice;
    final l = AppLocalizations.of(context);
    // The session manager's notice is a stored English constant: show its translation.
    final message = _incomplete
        ? l.sign_in_incomplete
        : _error != null
        ? messageFor(context, _error!)
        : (notice == sessionExpiredNotice ? l.error_unauthenticated : notice);
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.all(24),
              children: [
                const Center(child: PostitWordmark(height: 40)),
                const SizedBox(height: 32),
                Text(l.sign_in_title, style: Theme.of(context).textTheme.headlineSmall, textAlign: TextAlign.center),
                const SizedBox(height: 16),
                if (message != null) ...[
                  Text(message, key: const ValueKey('sign-in-error'), textAlign: TextAlign.center),
                  const SizedBox(height: 16),
                ],
                FilledButton(
                  onPressed: _busy ? null : _signIn,
                  child: _busy
                      ? const SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2))
                      : Text(l.action_sign_in),
                ),
                const SizedBox(height: 24),
                const Center(child: LanguageSwitch()),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
