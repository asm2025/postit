import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/theme.dart';

/// System, light and dark, as on the web's segmented control.
class ThemeControl extends ConsumerWidget {
  const ThemeControl({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
    return SegmentedButton<ThemeMode>(
      showSelectedIcon: false,
      segments: [
        ButtonSegment(
          value: ThemeMode.system,
          icon: const Icon(Icons.brightness_auto_outlined),
          tooltip: l.theme_system,
        ),
        ButtonSegment(value: ThemeMode.light, icon: const Icon(Icons.light_mode_outlined), tooltip: l.theme_light),
        ButtonSegment(value: ThemeMode.dark, icon: const Icon(Icons.dark_mode_outlined), tooltip: l.theme_dark),
      ],
      selected: {ref.watch(themeModeProvider)},
      onSelectionChanged: (s) => ref.read(themeModeProvider.notifier).set(s.single),
    );
  }
}
