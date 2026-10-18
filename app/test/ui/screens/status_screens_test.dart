import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/auth/session_store.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/status_screens.dart';
import 'package:postit/ui/widgets/logo.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';

class _PrefsMe implements MeRepository {
  final saved = <String>[];
  @override
  Future<MeDto> get() async => fakeMe(StatusDto.pending);
  @override
  Future<void> delete(String displayName) async {}
  @override
  Future<PreferencesDto> getPreferences() async =>
      PreferencesDto(locale: LocaleDto.fromJson(saved.isEmpty ? 'en-US' : saved.last));
  @override
  Future<PreferencesDto> putPreferences(String locale) async {
    saved.add(locale);
    return PreferencesDto(locale: LocaleDto.fromJson(locale));
  }
}

void main() {
  testWidgets('the loading screen continues the black startup screen', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: StartupScreen()));
    final scaffold = tester.widget<Scaffold>(find.byType(Scaffold));
    expect(scaffold.backgroundColor, const Color(0xFF231F20));
    expect(find.byType(PostitMark), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
  });

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

  testWidgets('sign in again clears the stored session', (tester) async {
    final store = MemoryStore(const StoredSession(refreshToken: 'kept', idToken: 'id-kept'));
    final client = FakeAuthClient()..onRefresh = (_) async => throw const AuthUnavailable();
    await pumpApp(
      tester,
      overrides: [
        sessionStoreProvider.overrideWithValue(store),
        sessionManagerProvider.overrideWith((ref) async {
          final m = SessionManager(client: client, store: ref.watch(sessionStoreProvider), revoke: (_) async {});
          await m.restore();
          return m;
        }),
      ],
    );
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
    await tester.tap(find.text('Sign in again'));
    await tester.pumpAndSettle();
    expect(store.value, isNull);
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('a startup error that is not the identity provider offers only retry', (tester) async {
    await pumpApp(
      tester,
      overrides: [sessionManagerProvider.overrideWith((ref) async => throw const ApiError(0, 'network'))],
    );
    expect(find.text('Retry'), findsOneWidget);
    expect(find.text('Sign in again'), findsNothing);
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
  testWidgets('the pending screen saves the chosen language to the account', (tester) async {
    final manager = await signedInManager();
    final me = _PrefsMe();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meRepositoryProvider.overrideWithValue(me),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.pending)),
      ],
    );
    await tester.tap(find.text('العربية'));
    await tester.pumpAndSettle();
    expect(me.saved, ['ar']);
    final container = ProviderScope.containerOf(tester.element(find.byType(StatusLayout)));
    expect(container.read(localeProvider), AppLocale.ar);
    // The next launch starts from the device language and reads the saved value back.
    await container.read(localeProvider.notifier).set(AppLocale.en);
    await container.read(localeProvider.notifier).syncFromServer();
    expect(container.read(localeProvider), AppLocale.ar);
  });

  testWidgets('the unavailable screen keeps the chosen language on the device only', (tester) async {
    final manager = await signedInManager();
    final me = _PrefsMe();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meRepositoryProvider.overrideWithValue(me),
        meProvider.overrideWith((ref) async => throw const ApiError(403, 'account_disabled')),
      ],
    );
    await tester.tap(find.text('العربية'));
    await tester.pumpAndSettle();
    expect(me.saved, isEmpty);
    final container = ProviderScope.containerOf(tester.element(find.byType(StatusLayout)));
    expect(container.read(localeProvider), AppLocale.ar);
  });
}
