import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/state/locale.dart';

/// Opens the language menu and picks [locale] from it.
Future<void> pickLanguage(WidgetTester tester, AppLocale locale) async {
  await tester.tap(find.byType(DropdownButton<AppLocale>));
  await tester.pumpAndSettle();
  await tester.tap(find.text(locale.nativeName).last);
}
