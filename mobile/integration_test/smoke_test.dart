import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:postit/main.dart' as app;

/// Opt-in, not in CI: needs the dev stack (./stack.ps1 up, cargo run) and a prepared AVD.
/// After "Sign in" opens Chrome, sign in as admin@postit.com by hand within two minutes.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('signs in through Zitadel and reaches Home', (tester) async {
    unawaited(app.main());
    await tester.pumpAndSettle(const Duration(seconds: 5));
    await tester.tap(find.text('Sign in'));
    final deadline = DateTime.now().add(const Duration(minutes: 2));
    while (find.text('Profile').evaluate().isEmpty && DateTime.now().isBefore(deadline)) {
      await tester.pump(const Duration(seconds: 1));
    }
    expect(find.text('Profile'), findsOneWidget);
    expect(find.textContaining('Good '), findsOneWidget);

    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Sign out'));
    final signedOut = DateTime.now().add(const Duration(minutes: 1));
    while (find.text('Sign in').evaluate().isEmpty && DateTime.now().isBefore(signedOut)) {
      await tester.pump(const Duration(seconds: 1));
    }
    expect(find.text('Sign in'), findsOneWidget);
  });
}
