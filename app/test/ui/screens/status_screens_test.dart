import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

void main() {
  testWidgets('startup failure shows retry, and retry recovers', (tester) async {
    var attempts = 0;
    final manager = await signedOutManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async {
          attempts++;
          if (attempts == 1) throw const ApiError(0, 'network');
          return manager;
        }),
      ],
    );
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('an unreachable provider at startup shows retry', (tester) async {
    await pumpApp(
      tester,
      overrides: [sessionManagerProvider.overrideWith((ref) async => throw const AuthUnavailable())],
    );
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
  });

  testWidgets('a platform without a client shows not configured', (tester) async {
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => null)]);
    expect(find.text('Mobile sign-in is not configured'), findsOneWidget);
  });

  testWidgets('pending shows refresh and sign-out', (tester) async {
    final manager = await signedInManager();
    var fetches = 0;
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async {
          fetches++;
          return fakeMe(StatusDto.pending);
        }),
      ],
    );
    expect(find.text('Waiting for approval'), findsOneWidget);
    await tester.tap(find.text('Check again'));
    await tester.pumpAndSettle();
    expect(fetches, 2);
    await tester.tap(find.text('Sign out'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('a disabled account shows unavailable', (tester) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => throw const ApiError(403, 'account_disabled')),
      ],
    );
    expect(find.text('Account unavailable'), findsOneWidget);
  });
}
