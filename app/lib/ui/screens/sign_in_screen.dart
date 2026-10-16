import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/widgets/logo.dart';

class SignInScreen extends ConsumerStatefulWidget {
  const SignInScreen({super.key});

  @override
  ConsumerState<SignInScreen> createState() => _SignInScreenState();
}

class _SignInScreenState extends ConsumerState<SignInScreen> {
  bool _busy = false;
  String? _error;

  Future<void> _signIn() async {
    final manager = ref.read(sessionManagerProvider).value;
    if (manager == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await manager.signIn();
    } on AuthCancelled {
      // The user closed the browser: nothing to report.
    } on AuthFailed catch (e) {
      _error = e.message;
    } on Object catch (e) {
      // AuthUnavailable reads as "can't reach postit"; anything unexpected gets the generic
      // message rather than vanishing.
      _error = messageFor(e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final notice = ref.watch(sessionStateProvider).notice;
    final message = _error ?? notice;
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
                if (message != null) ...[
                  Text(message, key: const ValueKey('sign-in-error'), textAlign: TextAlign.center),
                  const SizedBox(height: 16),
                ],
                FilledButton(
                  onPressed: _busy ? null : _signIn,
                  child: _busy
                      ? const SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2))
                      : const Text('Sign in'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
