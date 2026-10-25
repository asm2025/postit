import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

Map<String, String> _read(String name) {
  final arb = jsonDecode(File('lib/l10n/$name').readAsStringSync()) as Map<String, Object?>;
  return {
    for (final e in arb.entries)
      if (!e.key.startsWith('@') && e.value is String) e.key: e.value! as String,
  };
}

/// Plain `app_en.arb` messages whose key starts with one of [prefixes]. Messages with placeholders
/// or plural forms are left out: they never appear verbatim on screen.
Map<String, String> englishMessages(Iterable<String> prefixes) => {
  for (final e in _read('app_en.arb').entries)
    if (prefixes.any(e.key.startsWith) && !e.value.contains('{')) e.key: e.value,
};

String arabicMessage(String key) => _read('app_ar.arb')[key]!;

/// Fails when any English catalog message for [prefixes] is on screen (call with the Arabic UI up).
void expectNoEnglish(Iterable<String> prefixes) {
  final english = englishMessages(prefixes);
  expect(english, isNotEmpty, reason: 'no messages for $prefixes');
  for (final e in english.entries) {
    expect(find.text(e.value), findsNothing, reason: '${e.key} is still English');
  }
}
