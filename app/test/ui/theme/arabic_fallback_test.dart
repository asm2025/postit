import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/ui/theme/app_theme.dart';

void main() {
  for (final brightness in Brightness.values) {
    test('every ${brightness.name} text style falls back to Noto Sans Arabic', () {
      final t = buildTheme(brightness).textTheme;
      final styles = {
        'displayLarge': t.displayLarge,
        'headlineMedium': t.headlineMedium,
        'titleLarge': t.titleLarge,
        'titleMedium': t.titleMedium,
        'bodyLarge': t.bodyLarge,
        'bodyMedium': t.bodyMedium,
        'bodySmall': t.bodySmall,
        'labelLarge': t.labelLarge,
        'labelSmall': t.labelSmall,
      };
      for (final MapEntry(:key, :value) in styles.entries) {
        expect(value?.fontFamilyFallback, contains('NotoSansArabic'), reason: key);
      }
    });
  }
}
