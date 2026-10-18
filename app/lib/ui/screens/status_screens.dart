import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/logo.dart';

/// Shared layout for the full-screen states before the shell.
class StatusLayout extends StatelessWidget {
  const StatusLayout({super.key, required this.title, required this.body, this.actions = const []});

  final String title;
  final String body;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) => Scaffold(
    body: SafeArea(
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.all(24),
            children: [
              const Center(child: PostitBadge()),
              const SizedBox(height: 24),
              Text(title, style: Theme.of(context).textTheme.headlineSmall, textAlign: TextAlign.center),
              const SizedBox(height: 12),
              Text(body, textAlign: TextAlign.center),
              const SizedBox(height: 24),
              for (final a in actions) Padding(padding: const EdgeInsets.only(bottom: 12), child: a),
            ],
          ),
        ),
      ),
    ),
  );
}

Future<void> _signOut(WidgetRef ref) async {
  await ref.read(sessionManagerProvider).value?.signOut();
}

class StartupScreen extends StatelessWidget {
  const StartupScreen({super.key});

  // Continues the native startup screen (flutter_native_splash in pubspec.yaml): the same
  // black and mark, so the switch to Flutter does not flash.
  @override
  Widget build(BuildContext context) => AnnotatedRegion<SystemUiOverlayStyle>(
    value: SystemUiOverlayStyle.light,
    child: Scaffold(
      backgroundColor: postitLogoBlack,
      body: Stack(
        alignment: Alignment.center,
        children: [
          const PostitMark(),
          Align(
            alignment: const Alignment(0, 0.5),
            child: CircularProgressIndicator(color: context.tokens.pink),
          ),
        ],
      ),
    ),
  );
}

class ErrorScreen extends ConsumerWidget {
  const ErrorScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final startup = ref.watch(sessionManagerProvider);
    final error = startup.error ?? ref.watch(meProvider).error;
    return StatusLayout(
      title: 'Something went wrong',
      body: error == null ? 'Try again.' : messageFor(context, error),
      actions: [
        FilledButton(
          onPressed: () => startup.hasError ? ref.invalidate(sessionManagerProvider) : ref.invalidate(meProvider),
          child: const Text('Retry'),
        ),
        if (startup.error is AuthUnavailable)
          TextButton(onPressed: () => _signInAgain(ref), child: const Text('Sign in again')),
      ],
    );
  }

  /// Drops the stored session the provider keeps refusing, so startup lands on Sign in.
  Future<void> _signInAgain(WidgetRef ref) async {
    try {
      await ref.read(sessionStoreProvider).clear();
    } on Object {
      // The next restore reads whatever is left; a failed wipe is no worse than before.
    }
    ref.invalidate(sessionManagerProvider);
  }
}

class NotConfiguredScreen extends StatelessWidget {
  const NotConfiguredScreen({super.key});

  @override
  Widget build(BuildContext context) => const StatusLayout(
    title: 'Mobile sign-in is not configured',
    body:
        'This postit server has no sign-in client for this platform yet. Use the web app, '
        'or ask your administrator to set it up.',
  );
}

class PendingScreen extends ConsumerWidget {
  const PendingScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => RefreshIndicator(
    onRefresh: () => ref.refresh(meProvider.future),
    child: StatusLayout(
      title: 'Waiting for approval',
      body: 'An administrator has been told about your account. You can use postit once they approve it.',
      actions: [
        FilledButton(onPressed: () => ref.invalidate(meProvider), child: const Text('Check again')),
        OutlinedButton(onPressed: () => _signOut(ref), child: const Text('Sign out')),
      ],
    ),
  );
}

class UnavailableScreen extends ConsumerWidget {
  const UnavailableScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => StatusLayout(
    title: 'Account unavailable',
    body: 'This account is disabled or being deleted. Contact an administrator.',
    actions: [OutlinedButton(onPressed: () => _signOut(ref), child: const Text('Sign out'))],
  );
}
