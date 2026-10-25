import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/theme.dart';
import 'package:postit/ui/router.dart';
import 'package:postit/ui/theme/app_theme.dart';

class PostitApp extends ConsumerWidget {
  const PostitApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // After sign-in the saved language wins over the device's.
    ref.listen(sessionStateProvider.select((s) => s.status), (previous, status) {
      if (status == SessionStatus.signedIn) ref.read(localeProvider.notifier).syncFromServer();
    });
    return MaterialApp.router(
      title: 'postit',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(Brightness.light),
      darkTheme: buildTheme(Brightness.dark),
      themeMode: ref.watch(themeModeProvider),
      themeAnimationDuration: Duration.zero,
      locale: ref.watch(localeProvider).flutterLocale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      routerConfig: ref.watch(routerProvider),
    );
  }
}
