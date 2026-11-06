import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/user_detail_screen.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

UserDto _withEmail(UserDto u, String email) => UserDto.fromJson({...u.toJson(), 'email': email});

void main() {
  late FakeUsersRepository users;

  Future<void> openDetail(WidgetTester tester, String name) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: RoleDto.admin)),
        usersRepositoryProvider.overrideWithValue(users),
      ],
    );
    await tester.tap(find.text('Users'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text(name));
    await tester.pumpAndSettle();
    expect(find.byType(UserDetailScreen), findsOneWidget);
  }

  TextDirection? directionOf(WidgetTester tester, String text) =>
      tester.widget<Text>(find.text(text).last).textDirection;

  setUp(() {
    users = FakeUsersRepository([
      fakeUser('p1', status: StatusDto.pending, name: 'Pat Pending'),
      _withEmail(fakeUser('a1', name: 'Ann Active'), 'ann@postit.test'),
      _withEmail(fakeUser('r1', name: 'דנה לוי'), 'דנה@postit.test'),
    ]);
  });

  testWidgets('email uses its own direction', (tester) async {
    await openDetail(tester, 'Ann Active');
    expect(Directionality.of(tester.element(find.byType(UserDetailScreen))), TextDirection.ltr);
    expect(directionOf(tester, 'ann@postit.test'), TextDirection.ltr);
    expect(directionOf(tester, 'Ann Active'), TextDirection.ltr);
  });

  testWidgets('a right-to-left name and email read right to left inside an English screen', (tester) async {
    await openDetail(tester, 'דנה לוי');
    expect(Directionality.of(tester.element(find.byType(UserDetailScreen))), TextDirection.ltr);
    expect(directionOf(tester, 'דנה לוי'), TextDirection.rtl);
    expect(directionOf(tester, 'דנה@postit.test'), TextDirection.rtl);
  });
}
