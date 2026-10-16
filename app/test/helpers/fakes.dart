import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_store.dart';

TokenSet tokens(String n, {DateTime? expiresAt, String? refreshToken, bool noRefresh = false}) => TokenSet(
  accessToken: 'access-$n',
  expiresAt: expiresAt ?? DateTime(2030),
  refreshToken: noRefresh ? null : (refreshToken ?? 'refresh-$n'),
  idToken: 'id-$n',
);

class FakeAuthClient implements AuthClient {
  Future<TokenSet> Function() onSignIn = () async => tokens('signin');
  Future<TokenSet> Function(String refreshToken) onRefresh = (rt) async => tokens('refreshed');
  Future<void> Function(String idToken) onEndSession = (_) async {};
  int refreshCalls = 0;
  final refreshedWith = <String>[];
  final endedSessions = <String>[];

  @override
  Future<TokenSet> signIn() => onSignIn();

  @override
  Future<TokenSet> refresh(String refreshToken) {
    refreshCalls++;
    refreshedWith.add(refreshToken);
    return onRefresh(refreshToken);
  }

  @override
  Future<void> endSession(String idTokenHint) {
    endedSessions.add(idTokenHint);
    return onEndSession(idTokenHint);
  }
}

class MemoryStore implements SessionStore {
  MemoryStore([this.value]);
  StoredSession? value;
  bool failReads = false;

  @override
  Future<StoredSession?> read() async {
    if (failReads) throw StateError('keystore unavailable');
    return value;
  }

  @override
  Future<void> write(StoredSession session) async => value = session;

  @override
  Future<void> clear() async => value = null;
}
