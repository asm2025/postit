import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/profile_screen.dart';

import '../../helpers/arb.dart';
import '../../helpers/fake_users.dart';
import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';

class _FakeMe implements MeRepository {
  final deleted = <String>[];
  @override
  Future<MeDto> get() async => fakeMe(StatusDto.active);
  @override
  Future<void> delete(String displayName) async => deleted.add(displayName);
  @override
  Future<PreferencesDto> getPreferences() async => const PreferencesDto(locale: LocaleDto.enUs);
  final saved = <String>[];
  @override
  Future<PreferencesDto> putPreferences(String locale) async {
    saved.add(locale);
    return PreferencesDto(locale: LocaleDto.fromJson(locale));
  }
}

/// A store whose clear waits until the test releases it, so sign-out stays in progress.
class _SlowClearStore extends MemoryStore {
  final release = Completer<void>();
  var clears = 0;

  @override
  Future<void> clear() async {
    clears++;
    await release.future;
    return super.clear();
  }
}

void main() {
  testWidgets('sign out cannot be started twice', (tester) async {
    final store = _SlowClearStore();
    final manager = SessionManager(client: FakeAuthClient(), store: store, revoke: (_) async {});
    await manager.signIn();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
      ],
    );
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    final signOut = find.widgetWithText(OutlinedButton, 'Sign out');
    await tester.tap(signOut);
    await tester.pump();
    expect(tester.widget<OutlinedButton>(signOut).onPressed, isNull);
    await tester.tap(signOut, warnIfMissed: false);
    store.release.complete();
    await tester.pumpAndSettle();
    expect(store.clears, 1);
  });

  testWidgets('deleting the account needs the exact display name, then signs out', (tester) async {
    final client = FakeAuthClient();
    final manager = await signedInManager(client);
    final meRepo = _FakeMe();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
        meRepositoryProvider.overrideWithValue(meRepo),
      ],
    );
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.text('Delete my postit account'), 200);
    await tester.tap(find.text('Delete my postit account'));
    await tester.pumpAndSettle();

    final confirm = find.widgetWithText(FilledButton, 'Delete account');
    await tester.enterText(find.byType(TextField), 'ada');
    await tester.pump();
    expect(tester.widget<FilledButton>(confirm).onPressed, isNull);

    await tester.enterText(find.byType(TextField), 'Ada');
    await tester.pump();
    await tester.tap(confirm);
    await tester.pumpAndSettle();
    expect(meRepo.deleted, ['Ada']);
    expect(client.endedSessions, ['id-signin']);
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('sign out returns to sign-in', (tester) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
      ],
    );
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Sign out'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('changing language saves it', (tester) async {
    final manager = await signedInManager();
    final meRepo = _FakeMe();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
        meRepositoryProvider.overrideWithValue(meRepo),
      ],
    );
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('العربية'));
    await tester.pumpAndSettle();
    expect(meRepo.saved, ['ar']);
    expect(
      find.descendant(of: find.byType(ProfileScreen), matching: find.text(arabicMessage('profile_title'))),
      findsOneWidget,
    );
  });

  testWidgets('the Arabic profile shows no English catalog text', (tester) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
        deviceLocaleProvider.overrideWithValue(AppLocale.ar),
      ],
    );
    await tester.tap(find.text(arabicMessage('nav_profile')));
    await tester.pumpAndSettle();
    expectNoEnglish(['profile_', 'nav_', 'theme_', 'role_', 'action_sign_out', 'language_label']);
  });
}
