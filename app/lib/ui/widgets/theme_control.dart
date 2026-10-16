import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/theme.dart';

/// System, light and dark, as on the web's segmented control.
class ThemeControl extends ConsumerWidget {
  const ThemeControl({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => SegmentedButton<ThemeMode>(
    showSelectedIcon: false,
    segments: const [
      ButtonSegment(value: ThemeMode.system, icon: Icon(Icons.brightness_auto_outlined), tooltip: 'System'),
      ButtonSegment(value: ThemeMode.light, icon: Icon(Icons.light_mode_outlined), tooltip: 'Light'),
      ButtonSegment(value: ThemeMode.dark, icon: Icon(Icons.dark_mode_outlined), tooltip: 'Dark'),
    ],
    selected: {ref.watch(themeModeProvider)},
    onSelectionChanged: (s) => ref.read(themeModeProvider.notifier).set(s.single),
  );
}
