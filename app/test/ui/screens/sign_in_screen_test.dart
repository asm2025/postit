import 'package:flutter/widgets.dart' show ValueKey;
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';

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
}
