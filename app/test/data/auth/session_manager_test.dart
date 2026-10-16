import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/auth/session_store.dart';

import '../../helpers/fakes.dart';

void main() {
  late FakeAuthClient client;
  late MemoryStore store;
  late List<String> revoked;
  var now = DateTime(2030, 1, 1, 12);

  SessionManager manager({Future<void> Function(String)? revoke}) =>
      SessionManager(client: client, store: store, revoke: revoke ?? (rt) async => revoked.add(rt), now: () => now);

  setUp(() {
    client = FakeAuthClient();
    store = MemoryStore();
    revoked = [];
    now = DateTime(2030, 1, 1, 12);
  });

  test('restore with nothing stored is signed out without a notice', () async {
    final m = manager();
    await m.restore();
    expect(m.state.value, const SessionState.signedOut());
    expect(client.refreshCalls, 0);
  });

  test('restore refreshes the stored token and signs in', () async {
    store.value = const StoredSession(refreshToken: 'stored', idToken: 'id-stored');
    final m = manager();
    await m.restore();
    expect(client.refreshedWith, ['stored']);
    expect(m.state.value, const SessionState.signedIn());
    expect(await m.accessToken(), 'access-refreshed');
  });

  test('restore persists a rotated refresh token', () async {
    store.value = const StoredSession(refreshToken: 'old');
    client.onRefresh = (_) async => tokens('r', refreshToken: 'rotated');
    await manager().restore();
    expect(store.value?.refreshToken, 'rotated');
  });

  test('a refresh without a new refresh token keeps the old one', () async {
    store.value = const StoredSession(refreshToken: 'old');
    client.onRefresh = (_) async => tokens('r', noRefresh: true);
    await manager().restore();
    expect(store.value?.refreshToken, 'old');
  });

  test('restore that fails to refresh is signed out silently and clears the store', () async {
    store.value = const StoredSession(refreshToken: 'revoked');
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    final m = manager();
    await m.restore();
    expect(m.state.value, const SessionState.signedOut());
    expect(store.value, isNull);
  });

  test('restore that cannot reach the provider keeps the stored session and fails', () async {
    store.value = const StoredSession(refreshToken: 'stored', idToken: 'id-stored');
    client.onRefresh = (_) async => throw const AuthUnavailable();
    final m = manager();
    await expectLater(m.restore(), throwsA(isA<AuthUnavailable>()));
    expect(store.value?.refreshToken, 'stored');
    // A retry (the error screen rebuilds the manager) restores normally.
    client.onRefresh = (_) async => tokens('later');
    final again = manager();
    await again.restore();
    expect(again.state.value, const SessionState.signedIn());
  });

  test('unreadable store means signed out', () async {
    store.failReads = true;
    final m = manager();
    await m.restore();
    expect(m.state.value.status, SessionStatus.signedOut);
  });

  test('sign-in stores the refresh token and signs in', () async {
    final m = manager();
    await m.restore();
    await m.signIn();
    expect(m.state.value, const SessionState.signedIn());
    expect(store.value?.refreshToken, 'refresh-signin');
    expect(store.value?.idToken, 'id-signin');
  });

  test('cancelled sign-in stays signed out', () async {
    client.onSignIn = () async => throw const AuthCancelled();
    final m = manager();
    await m.restore();
    await expectLater(m.signIn(), throwsA(isA<AuthCancelled>()));
    expect(m.state.value, const SessionState.signedOut());
  });

  test('concurrent refreshes share one call', () async {
    final m = manager();
    await m.signIn();
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final all = Future.wait([m.refresh(), m.refresh(), m.refresh()]);
    gate.complete(tokens('shared'));
    expect(await all, ['access-shared', 'access-shared', 'access-shared']);
    expect(client.refreshCalls, 1);
  });

  test('the access token is refreshed within 60 seconds of expiry', () async {
    client.onSignIn = () async => tokens('a', expiresAt: now.add(const Duration(seconds: 30)));
    final m = manager();
    await m.signIn();
    expect(await m.accessToken(), 'access-refreshed');
    expect(client.refreshCalls, 1);
  });

  test('a fresh access token is used as is', () async {
    client.onSignIn = () async => tokens('a', expiresAt: now.add(const Duration(minutes: 10)));
    final m = manager();
    await m.signIn();
    expect(await m.accessToken(), 'access-a');
    expect(client.refreshCalls, 0);
  });

  test('a failed refresh expires the session with a notice', () async {
    final m = manager();
    await m.signIn();
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    await expectLater(m.refresh(), throwsA(isA<NotSignedIn>()));
    expect(m.state.value, const SessionState.signedOut(sessionExpiredNotice));
    expect(store.value, isNull);
  });

  test('a refresh that cannot reach the provider keeps the session', () async {
    final m = manager();
    await m.signIn();
    client.onRefresh = (_) async => throw const AuthUnavailable();
    await expectLater(m.refresh(), throwsA(isA<AuthUnavailable>()));
    expect(m.state.value, const SessionState.signedIn());
    expect(store.value?.refreshToken, 'refresh-signin');
    client.onRefresh = (_) async => tokens('back');
    expect(await m.refresh(), 'access-back');
  });

  test('signing out during a refresh does not bring the session back', () async {
    final m = manager();
    await m.signIn();
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final inFlight = m.refresh();
    await m.signOut();
    gate.complete(tokens('late', refreshToken: 'late-refresh'));
    await expectLater(inFlight, throwsA(isA<NotSignedIn>()));
    expect(m.state.value, const SessionState.signedOut());
    expect(store.value, isNull);
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });

  test('a session saved while signing out is cleared again', () async {
    final writing = Completer<void>();
    final slow = _SlowStore(writing.future);
    final m = SessionManager(client: client, store: slow, revoke: (_) async {}, now: () => now);
    final signingIn = m.signIn();
    await Future<void>.delayed(Duration.zero); // signIn is now blocked in the store write
    await m.signOut();
    writing.complete();
    await expectLater(signingIn, throwsA(isA<NotSignedIn>()));
    expect(slow.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
  });

  test('accessToken without a session throws NotSignedIn', () async {
    final m = manager();
    await m.restore();
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });

  test('sign-out revokes, ends the session and clears', () async {
    final m = manager();
    await m.signIn();
    await m.signOut();
    expect(revoked, ['refresh-signin']);
    expect(client.endedSessions, ['id-signin']);
    expect(store.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
  });

  test('sign-out clears even when revocation fails and the browser step is cancelled', () async {
    final m = manager(revoke: (_) async => throw StateError('offline'));
    client.onEndSession = (_) async => throw const AuthCancelled();
    await m.signIn();
    await m.signOut();
    expect(store.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });
}

/// A store whose writes wait for [_gate], to hold a save open while the user signs out.
class _SlowStore extends MemoryStore {
  _SlowStore(this._gate);
  final Future<void> _gate;

  @override
  Future<void> write(StoredSession session) async {
    await _gate;
    value = session;
  }
}
