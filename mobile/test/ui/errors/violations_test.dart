import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/ui/errors/violations.dart';

Future<BuildContext> _context(WidgetTester tester) async {
  late BuildContext captured;
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('en'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: Builder(
        builder: (context) {
          captured = context;
          return const SizedBox();
        },
      ),
    ),
  );
  return captured;
}

void main() {
  testWidgets('every violation code reads as a sentence, even without parameters', (tester) async {
    final context = await _context(tester);
    for (final code in ViolationCode.values) {
      // An agent's own rule is its message; it has nothing to say without one.
      final params = code == ViolationCode.agentRule
          ? {
              'message': {'en': 'A rule.'},
            }
          : null;
      final text = violationMessage(context, Violation(code: code, params: params));
      expect(text.trim(), isNotEmpty, reason: code.name);
    }
  });

  testWidgets('parameters fill the text', (tester) async {
    final context = await _context(tester);
    final text = violationMessage(
      context,
      const Violation(code: ViolationCode.textTooLong, params: {'max': 280, 'actual': 301, 'measure': 'graphemes'}),
    );
    expect(text, 'The text is 301 characters; this platform allows 280.');
    final rule = violationMessage(
      context,
      const Violation(
        code: ViolationCode.agentRule,
        params: {
          'message': {'key': 'x', 'en': 'Use fewer tags.'},
        },
      ),
    );
    expect(rule, 'Use fewer tags.');
    final list = violationMessage(
      context,
      const Violation(
        code: ViolationCode.mediaKindNotAllowed,
        params: {
          'kind': 'video',
          'allowed': ['image', 'audio'],
        },
      ),
    );
    expect(list, contains('image, audio'));
  });
}
