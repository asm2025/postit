import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/gate.dart';

import '../helpers/fake_users.dart';
import '../helpers/fakes.dart';

void main() {
  final manager = SessionManager(client: FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  const signedIn = SessionState.signedIn();

  GateTarget target({
    AsyncValue<SessionManager?>? startup,
    SessionState session = signedIn,
    AsyncValue<MeDto>? meValue,
  }) => gateTarget(
    startup: startup ?? AsyncData(manager),
    session: session,
    me: meValue ?? AsyncData(fakeMe(StatusDto.active)),
  );

  test('startup states', () {
    expect(target(startup: const AsyncLoading()), GateTarget.startup);
    expect(target(startup: AsyncError(const ApiError(0, 'network'), StackTrace.empty)), GateTarget.error);
    expect(target(startup: const AsyncData(null)), GateTarget.notConfigured);
  });

  test('session states', () {
    expect(target(session: const SessionState.loading()), GateTarget.startup);
    expect(target(session: const SessionState.signedOut()), GateTarget.signIn);
  });

  test('me states', () {
    expect(target(meValue: const AsyncLoading()), GateTarget.startup);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.pending))), GateTarget.pending);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.disabled))), GateTarget.unavailable);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.deleting))), GateTarget.unavailable);
    expect(target(), GateTarget.shell);
    expect(
      target(meValue: AsyncError(const ApiError(403, 'account_disabled'), StackTrace.empty)),
      GateTarget.unavailable,
    );
    expect(target(meValue: AsyncError(const ApiError(403, 'account_pending'), StackTrace.empty)), GateTarget.pending);
    expect(target(meValue: AsyncError(const ApiError(0, 'network'), StackTrace.empty)), GateTarget.error);
  });

  test('redirects', () {
    expect(redirectFor(GateTarget.signIn, '/home', isAdmin: false), '/sign-in');
    expect(redirectFor(GateTarget.signIn, '/sign-in', isAdmin: false), isNull);
    expect(redirectFor(GateTarget.shell, '/sign-in', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/profile', isAdmin: false), isNull);
    expect(redirectFor(GateTarget.shell, '/users', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/users/u2', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/users/u2', isAdmin: true), isNull);
  });
}
