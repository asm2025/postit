import 'dart:math';
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:postit/ui/theme/tokens.dart';

double contrast(Color a, Color b) {
  final la = a.computeLuminance();
  final lb = b.computeLuminance();
  return (max(la, lb) + 0.05) / (min(la, lb) + 0.05);
}

void main() {
  for (final (name, t) in [('light', PostitTokens.light), ('dark', PostitTokens.dark)]) {
    test('$name text meets WCAG AA', () {
      for (final bg in [t.bg, t.surface, t.raised]) {
        expect(contrast(t.text, bg), greaterThanOrEqualTo(4.5));
        expect(contrast(t.mutedText, bg), greaterThanOrEqualTo(4.5));
        expect(contrast(t.pinkText, bg), greaterThanOrEqualTo(4.5));
      }
      expect(contrast(t.onPink, t.pink), greaterThanOrEqualTo(4.5));
      expect(contrast(t.warnText, t.warnBg), greaterThanOrEqualTo(4.5));
    });

    test('$name focus indicator reaches 3:1', () {
      for (final bg in [t.bg, t.surface, t.raised]) {
        expect(contrast(t.pinkText, bg), greaterThanOrEqualTo(3));
      }
    });
  }
}
