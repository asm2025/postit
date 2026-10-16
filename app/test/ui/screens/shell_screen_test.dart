import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

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
    expect(find.text('Good morning, Ada'), findsOneWidget);
    expect(find.textContaining('only active admin'), findsOneWidget);
    expect(find.text('Review'), findsOneWidget);
  });
}
