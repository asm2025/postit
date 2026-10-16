import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/theme.dart';
import 'package:postit/ui/router.dart';
import 'package:postit/ui/theme/app_theme.dart';

class PostitApp extends ConsumerWidget {
  const PostitApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => MaterialApp.router(
    title: 'postit',
    debugShowCheckedModeBanner: false,
    theme: buildTheme(Brightness.light),
    darkTheme: buildTheme(Brightness.dark),
    themeMode: ref.watch(themeModeProvider),
    themeAnimationDuration: Duration.zero,
    routerConfig: ref.watch(routerProvider),
  );
}
