import 'package:flutter/material.dart';
import 'package:postit/ui/theme/tokens.dart';

/// The platform's page transition, or none when the OS asks for reduced motion.
class ReducedMotionTransitions extends PageTransitionsBuilder {
  const ReducedMotionTransitions();

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) => MediaQuery.disableAnimationsOf(context)
      ? child
      // A PageTransitionsTheme without builders applies the platform defaults.
      : const PageTransitionsTheme().buildTransitions(route, context, animation, secondaryAnimation, child);
}

/// Variable fonts need the weight axis set as well as [FontWeight].
TextStyle weighted(TextStyle base, int weight) => base.copyWith(
  fontWeight: FontWeight.values[(weight ~/ 100 - 1).clamp(0, 8)],
  fontVariations: [FontVariation.weight(weight.toDouble())],
);

/// Latin faces carry no Arabic glyphs, so Arabic text falls through to Noto Sans Arabic.
const _arabicFallback = ['NotoSansArabic'];

ThemeData buildTheme(Brightness brightness) {
  final t = brightness == Brightness.light ? PostitTokens.light : PostitTokens.dark;
  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.pink,
    onPrimary: t.onPink,
    secondary: t.pinkText,
    onSecondary: t.surface,
    error: t.danger,
    onError: t.surface,
    surface: t.surface,
    onSurface: t.text,
    onSurfaceVariant: t.mutedText,
    surfaceContainerLowest: t.surface,
    surfaceContainerLow: t.surface,
    surfaceContainer: t.raised,
    surfaceContainerHigh: t.raised,
    surfaceContainerHighest: t.raised,
    outline: t.line,
    outlineVariant: t.line,
  );
  final base = ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: t.bg,
    fontFamily: 'Onest',
    fontFamilyFallback: _arabicFallback,
    materialTapTargetSize: MaterialTapTargetSize.padded,
    extensions: [t],
  );
  TextStyle heading(TextStyle? s, int w) => weighted(
    (s ?? const TextStyle()).copyWith(
      fontFamily: 'BricolageGrotesque',
      fontFamilyFallback: _arabicFallback,
      color: t.text,
    ),
    w,
  );
  TextStyle mono(TextStyle? s) => weighted(
    (s ?? const TextStyle()).copyWith(
      fontFamily: 'JetBrainsMono',
      fontFamilyFallback: _arabicFallback,
      color: t.mutedText,
    ),
    500,
  );
  final text = base.textTheme;
  return base.copyWith(
    textTheme: text.copyWith(
      displayLarge: heading(text.displayLarge, 700),
      displayMedium: heading(text.displayMedium, 700),
      displaySmall: heading(text.displaySmall, 700),
      headlineLarge: heading(text.headlineLarge, 700),
      headlineMedium: heading(text.headlineMedium, 650),
      headlineSmall: heading(text.headlineSmall, 650),
      titleLarge: heading(text.titleLarge, 600),
      labelSmall: mono(text.labelSmall),
    ),
    cardTheme: CardThemeData(
      color: t.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: t.line),
      ),
    ),
    dividerTheme: DividerThemeData(color: t.line),
    pageTransitionsTheme: PageTransitionsTheme(
      builders: {for (final p in TargetPlatform.values) p: const ReducedMotionTransitions()},
    ),
    focusColor: t.pinkSoft,
    navigationBarTheme: NavigationBarThemeData(backgroundColor: t.bg, indicatorColor: t.pinkSoft),
    navigationRailTheme: NavigationRailThemeData(backgroundColor: t.bg, indicatorColor: t.pinkSoft),
    inputDecorationTheme: InputDecorationTheme(
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(10)),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: t.pinkText, width: 2),
      ),
    ),
  );
}
