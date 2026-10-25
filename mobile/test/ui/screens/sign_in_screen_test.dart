import 'package:flutter/widgets.dart' show Directionality, TextDirection, ValueKey;
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/sign_in_screen.dart';

import '../../helpers/arb.dart';
import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';
import '../../helpers/language.dart';

void main() {
  testWidgets('cancelled sign-in shows no message', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthCancelled();
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
    expect(find.byKey(const ValueKey('sign-in-error')), findsNothing);
  });

  testWidgets('a provider rejection shows a plain message, not the provider text', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthFailed('Errors.Internal (QUERY-Dfbe2)');
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text("Sign-in didn't complete. Try again."), findsOneWidget);
    expect(find.textContaining('QUERY'), findsNothing);
  });

  testWidgets('an unreachable provider says so', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthUnavailable();
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
  });

  testWidgets('an unexpected failure is not swallowed', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw StateError('boom');
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text('Something went wrong. Try again.'), findsOneWidget);
  });

  testWidgets('an expired session explains itself', (tester) async {
    final manager = await signedInManager();
    await manager.expire();
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    expect(find.text(sessionExpiredNotice), findsOneWidget);
  });

  testWidgets('switching to Arabic turns the screen RTL', (tester) async {
    final manager = await signedOutManager();
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    final screen = find.byType(SignInScreen);
    expect(Directionality.of(tester.element(screen)), TextDirection.ltr);
    await pickLanguage(tester, AppLocale.ar);
    await tester.pumpAndSettle();
    expect(Directionality.of(tester.element(screen)), TextDirection.rtl);
    expect(find.text(arabicMessage('sign_in_title')), findsOneWidget);
    expect(find.text(arabicMessage('action_sign_in')), findsOneWidget);
  });

  testWidgets('the Arabic sign-in screen shows no English catalog text', (tester) async {
    final manager = await signedOutManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        deviceLocaleProvider.overrideWithValue(AppLocale.ar),
      ],
    );
    expect(find.text(arabicMessage('sign_in_title')), findsOneWidget);
    expectNoEnglish(['sign_in_', 'action_sign_in', 'language_label']);
  });
}
