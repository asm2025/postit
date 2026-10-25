import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';
import '../../helpers/isolates.dart';

void main() {
  Future<void> pumpShell(WidgetTester tester, {RoleDto role = RoleDto.member, Size size = const Size(400, 800)}) async {
    final manager = await signedInManager();
    final users = FakeUsersRepository([fakeUser('p1', status: StatusDto.pending), fakeUser('a1', role: RoleDto.admin)]);
    await pumpApp(
      tester,
      size: size,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: role)),
        usersRepositoryProvider.overrideWithValue(users),
      ],
    );
  }

  testWidgets('members get Home and Profile on a bottom bar', (tester) async {
    await pumpShell(tester);
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.text('Home'), findsWidgets);
    expect(find.text('Profile'), findsOneWidget);
    expect(find.text('Users'), findsNothing);
  });

  testWidgets('admins also get Users with the pending badge', (tester) async {
    await pumpShell(tester, role: RoleDto.admin);
    expect(find.text('Users'), findsOneWidget);
    expect(find.descendant(of: find.byType(Badge), matching: find.text('1')), findsOneWidget);
  });

  testWidgets('wide screens use a navigation rail', (tester) async {
    await pumpShell(tester, size: const Size(900, 700));
    expect(find.byType(NavigationRail), findsOneWidget);
    expect(find.byType(NavigationBar), findsNothing);
  });

  testWidgets('home greets by time of day and warns the only admin', (tester) async {
    await pumpShell(tester, role: RoleDto.admin);
    expect(findIsolatedText('Good morning, Ada'), findsOneWidget);
    expect(find.textContaining('only active admin'), findsOneWidget);
    expect(find.text('Review'), findsOneWidget);
  });

  Future<void> greeting(WidgetTester tester, String name, AppLocale locale) async {
    final manager = await signedInManager();
    final me = MeDto.fromJson({...fakeMe(StatusDto.active).toJson(), 'display_name': name});
    await pumpApp(
      tester,
      overrides: [
        deviceLocaleProvider.overrideWithValue(locale),
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => me),
      ],
    );
  }

  testWidgets('an Arabic name in an English sentence is isolated', (tester) async {
    await greeting(tester, 'علي', AppLocale.en);
    expect(find.text('Good morning, \u2068علي\u2069'), findsOneWidget);
  });

  testWidgets('a Latin name in an Arabic sentence is isolated', (tester) async {
    await greeting(tester, 'Ada', AppLocale.ar);
    expect(find.text('صباح الخير، \u2068Ada\u2069'), findsOneWidget);
  });
}
