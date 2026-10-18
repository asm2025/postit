import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/format.dart';

Map<String, Object?> _arb(String name) => jsonDecode(File('lib/l10n/$name').readAsStringSync()) as Map<String, Object?>;

Set<String> _keys(Map<String, Object?> arb) => arb.keys.where((k) => !k.startsWith('@')).toSet();

Future<BuildContext> _pump(WidgetTester tester, String language) async {
  late BuildContext captured;
  await tester.pumpWidget(
    MaterialApp(
      locale: Locale(language),
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
  test('arb files have identical keys', () {
    final en = _arb('app_en.arb');
    final ar = _arb('app_ar.arb');
    expect(_keys(ar), _keys(en));
    for (final arb in [en, ar]) {
      for (final key in _keys(arb)) {
        expect(arb[key], isA<String>().having((s) => s.trim(), 'trimmed', isNotEmpty), reason: key);
      }
    }
  });

  test('every ErrorCode has a translation', () {
    final spec = jsonDecode(File('../api/openapi.json').readAsStringSync()) as Map<String, Object?>;
    final schemas = (spec['components'] as Map<String, Object?>)['schemas'] as Map<String, Object?>;
    final codes = ((schemas['ErrorCode'] as Map<String, Object?>)['enum'] as List<Object?>).cast<String>();
    expect(codes, isNotEmpty);
    for (final arb in [_arb('app_en.arb'), _arb('app_ar.arb')]) {
      for (final code in codes) {
        expect(arb, contains('error_$code'));
      }
      expect(arb, contains('error_generic'));
    }
  });

  testWidgets('Arabic digits', (tester) async {
    final context = await _pump(tester, 'ar');
    expect(formatNumber(context, 1234), '١٬٢٣٤');
    expect(formatNumber(context, 0), '٠');
  });

  testWidgets('English digits stay Western', (tester) async {
    final context = await _pump(tester, 'en');
    expect(formatNumber(context, 1234), '1,234');
  });

  testWidgets('dates and relative times use Arabic digits in Arabic', (tester) async {
    final context = await _pump(tester, 'ar');
    final now = DateTime(2026, 10, 18, 12);
    final date = formatDate(context, DateTime(2026, 10, 18), pattern: 'yyyy-MM-dd');
    expect(date, '٢٠٢٦-١٠-١٨');
    final relative = formatRelative(context, now.subtract(const Duration(minutes: 5)), now: now);
    expect(relative, matches(RegExp(r'^[^0-9]*[٠-٩]+[^0-9]*$')));
    expect(formatRelative(context, now, now: now), AppLocalizations.of(context).relative_just_now);
  });

  testWidgets('relative times in English', (tester) async {
    final context = await _pump(tester, 'en');
    final now = DateTime(2026, 10, 18, 12);
    expect(formatRelative(context, now.subtract(const Duration(minutes: 5)), now: now), '5 minutes ago');
    expect(formatRelative(context, now.subtract(const Duration(hours: 1)), now: now), '1 hour ago');
    expect(formatRelative(context, now.subtract(const Duration(days: 3)), now: now), '3 days ago');
  });

  testWidgets('known error code is translated', (tester) async {
    final context = await _pump(tester, 'ar');
    expect(messageFor(context, const ApiError(409, 'last_admin')), AppLocalizations.of(context).error_last_admin);
  });

  testWidgets('unknown error code falls back', (tester) async {
    final context = await _pump(tester, 'ar');
    final l = AppLocalizations.of(context);
    final message = messageFor(context, const ApiError(418, 'teapot', 'Short and stout.'));
    expect(message, l.error_generic);
    expect(message, isNot(contains('Short')));
  });
}
