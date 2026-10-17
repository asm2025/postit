import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

void main() {
  late FakeUsersRepository users;

  Future<void> openUsers(WidgetTester tester, {Size size = const Size(400, 800)}) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      size: size,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: RoleDto.admin)),
        usersRepositoryProvider.overrideWithValue(users),
      ],
    );
    await tester.tap(find.text('Users'));
    await tester.pumpAndSettle();
  }

  setUp(() {
    users = FakeUsersRepository([
      fakeUser('p1', status: StatusDto.pending, name: 'Pat Pending'),
      fakeUser('a1', name: 'Ann Active'),
      fakeUser('d1', status: StatusDto.disabled, name: 'Dan Disabled'),
    ]);
  });

  testWidgets('starts on pending when someone is waiting', (tester) async {
    await openUsers(tester);
    expect(find.text('Pat Pending'), findsOneWidget);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('approving a pending user', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Pat Pending'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Approve'));
    await tester.pumpAndSettle();
    expect(users.calls, ['approve p1']);
    expect(find.text('Disable'), findsOneWidget);
  });

  testWidgets('disabling asks for confirmation and shows last_admin', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    users.failWith = const ApiError(409, 'last_admin');
    await tester.tap(find.text('Disable'));
    await tester.pumpAndSettle();
    expect(find.text('Disable Ann Active?'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Disable'));
    await tester.pumpAndSettle();
    expect(find.text('That would remove the last active admin.'), findsOneWidget);
  });

  testWidgets('rejecting a pending user deletes them and returns to the list', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Pat Pending'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Reject'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Reject'));
    await tester.pumpAndSettle();
    expect(users.calls, ['delete p1']);
    expect(find.text('Pat Pending'), findsNothing);
  });

  testWidgets('search filters by name', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'dan');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    expect(find.text('Dan Disabled'), findsOneWidget);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('scrolling to the end loads the next page', (tester) async {
    users = FakeUsersRepository(List.generate(40, (i) => fakeUser('u$i', name: 'User $i')));
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    // The search field and the chip row are scrollables too; the list is the last one.
    await tester.scrollUntilVisible(find.text('User 39'), 400, scrollable: find.byType(Scrollable).last);
    expect(users.pagesRequested, containsAllInOrder([1, 2]));
  });

  testWidgets('a failed next page keeps the loaded users and offers a retry', (tester) async {
    users = FakeUsersRepository(List.generate(40, (i) => fakeUser('u$i', name: 'User $i')))..failPages.add(2);
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.text("Couldn't load more."), 400, scrollable: find.byType(Scrollable).last);
    await tester.pumpAndSettle();
    expect(find.text('User 24'), findsOneWidget);
    users.failPages.clear();
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.text('User 39'), 400, scrollable: find.byType(Scrollable).last);
    expect(find.text('User 39'), findsOneWidget);
  });

  testWidgets('a tall screen keeps loading until the list scrolls', (tester) async {
    users = FakeUsersRepository(List.generate(40, (i) => fakeUser('u$i', name: 'User $i')));
    await openUsers(tester, size: const Size(900, 4000));
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    expect(users.pagesRequested, containsAllInOrder([1, 2]));
  });

  testWidgets('enabling a disabled user needs no confirmation', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Disabled'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dan Disabled'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Enable'));
    await tester.pumpAndSettle();
    expect(users.calls, ['enable d1']);
    expect(find.text('Disable'), findsOneWidget);
  });

  testWidgets('making an admin is direct; making a member asks first', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Make admin'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1']);
    await tester.tap(find.text('Make member'));
    await tester.pumpAndSettle();
    expect(find.text('Make Ann Active a member?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1']);
    await tester.tap(find.text('Make member'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Make member'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1', 'role:member a1']);
  });

  testWidgets('deleting asks first and returns to the list', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();
    expect(find.text('Delete Ann Active?'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Delete'));
    await tester.pumpAndSettle();
    expect(users.calls, ['delete a1']);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('you cannot delete yourself from the detail screen', (tester) async {
    users.users.add(fakeUser('u1', name: 'Ada', role: RoleDto.admin));
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ada'));
    await tester.pumpAndSettle();
    expect(find.text('Delete'), findsNothing);
  });

  testWidgets('members never reach /users', (tester) async {
    final manager = await signedInManager();
    await pumpApp(
      tester,
      overrides: [
        sessionManagerProvider.overrideWith((ref) async => manager),
        meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
        usersRepositoryProvider.overrideWithValue(users),
      ],
    );
    expect(find.text('Users'), findsNothing);
  });
}
