import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';

class _FakeMe implements MeRepository {
  final deleted = <String>[];
  @override
  Future<MeDto> get() async => fakeMe(StatusDto.active);
  @override
  Future<void> delete(String displayName) async => deleted.add(displayName);
}

void main() {
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
}
