import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/language_switch.dart';
import 'package:postit/ui/widgets/logo.dart';

/// Shared layout for the full-screen states before the shell.
class StatusLayout extends StatelessWidget {
  const StatusLayout({
    super.key,
    required this.title,
    required this.body,
    this.actions = const [],
    this.languageSwitch = false,
    this.saveLanguage = false,
  });

  final String title;
  final String body;
  final List<Widget> actions;

  /// The pending and unavailable screens let people change language without signing in again.
  final bool languageSwitch;

  /// Also saves the chosen language to the account. Only for a screen the API accepts it from: the
  /// unavailable screen is shown because the account is refused, so its choice stays on the device.
  final bool saveLanguage;

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
              for (final a in actions) Padding(padding: const EdgeInsetsDirectional.only(bottom: 12), child: a),
              if (languageSwitch) Center(child: LanguageSwitch(persist: saveLanguage)),
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
    final l = AppLocalizations.of(context);
    final startup = ref.watch(sessionManagerProvider);
    final error = startup.error ?? ref.watch(meProvider).error;
    return StatusLayout(
      title: l.screen_error_title,
      body: error == null ? l.screen_error_body : messageFor(context, error),
      actions: [
        FilledButton(
          onPressed: () => startup.hasError ? ref.invalidate(sessionManagerProvider) : ref.invalidate(meProvider),
          child: Text(l.action_retry),
        ),
        if (startup.error is AuthUnavailable)
          TextButton(onPressed: () => _signInAgain(ref), child: Text(l.screen_sign_in_again)),
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
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return StatusLayout(title: l.screen_not_configured_title, body: l.screen_not_configured_body);
  }
}

class PendingScreen extends ConsumerWidget {
  const PendingScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
    return RefreshIndicator(
      onRefresh: () => ref.refresh(meProvider.future),
      child: StatusLayout(
        title: l.screen_pending_title,
        body: l.screen_pending_body,
        languageSwitch: true,
        saveLanguage: true,
        actions: [
          FilledButton(onPressed: () => ref.invalidate(meProvider), child: Text(l.screen_pending_check)),
          OutlinedButton(onPressed: () => _signOut(ref), child: Text(l.action_sign_out)),
        ],
      ),
    );
  }
}

class UnavailableScreen extends ConsumerWidget {
  const UnavailableScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
    return StatusLayout(
      title: l.screen_unavailable_title,
      body: l.screen_unavailable_body,
      languageSwitch: true,
      actions: [OutlinedButton(onPressed: () => _signOut(ref), child: Text(l.action_sign_out))],
    );
  }
}
