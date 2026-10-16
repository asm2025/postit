import 'package:flutter/foundation.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_store.dart';

enum SessionStatus { loading, signedOut, signedIn }

/// Where the session stands; [notice] explains a sign-out the user did not ask for.
@immutable
class SessionState {
  const SessionState._(this.status, [this.notice]);
  const SessionState.loading() : this._(SessionStatus.loading);
  const SessionState.signedOut([String? notice]) : this._(SessionStatus.signedOut, notice);
  const SessionState.signedIn() : this._(SessionStatus.signedIn);

  final SessionStatus status;
  final String? notice;

  @override
  bool operator ==(Object other) => other is SessionState && other.status == status && other.notice == notice;

  @override
  int get hashCode => Object.hash(status, notice);

  @override
  String toString() => 'SessionState($status, $notice)';
}

const sessionExpiredNotice = 'Your session expired. Sign in again.';

typedef RevokeToken = Future<void> Function(String refreshToken);

/// Owns the tokens: the access token in memory, the refresh and ID tokens in [SessionStore].
class SessionManager {
  SessionManager({required this._client, required this._store, required this._revoke, this._now = DateTime.now});

  final AuthClient _client;
  final SessionStore _store;
  final RevokeToken _revoke;
  final DateTime Function() _now;

  final ValueNotifier<SessionState> state = ValueNotifier(const SessionState.loading());

  /// Refresh this long before expiry, so a request never leaves with a dying token.
  static const refreshMargin = Duration(seconds: 60);

  String? _accessToken;
  DateTime? _expiresAt;
  String? _refreshToken;
  String? _idToken;
  Future<String>? _refreshing;

  /// Bumped by every wipe. A sign-in or refresh that started under an older generation
  /// finished after a sign-out, so its tokens are discarded instead of reviving the session.
  int _generation = 0;

  /// Resumes a stored session, or settles on signed out without a notice. Throws
  /// [AuthUnavailable], keeping the stored session, when the provider cannot be reached.
  Future<void> restore() async {
    StoredSession? stored;
    try {
      stored = await _store.read();
    } on Object {
      stored = null;
    }
    if (stored == null) {
      await _wipe();
      state.value = const SessionState.signedOut();
      return;
    }
    _refreshToken = stored.refreshToken;
    _idToken = stored.idToken;
    try {
      await _apply(await _client.refresh(stored.refreshToken));
    } on AuthUnavailable {
      // Offline or the provider is down: keep the stored session for the next try.
      _refreshToken = null;
      _idToken = null;
      rethrow;
    } on Object {
      await _wipe();
      state.value = const SessionState.signedOut();
    }
  }

  /// Opens the browser sign-in. Throws [AuthCancelled], [AuthFailed], or [AuthUnavailable],
  /// or [NotSignedIn] if a sign-out raced it.
  Future<void> signIn() async => _apply(await _client.signIn());

  /// A usable access token, refreshed first when it is close to expiry.
  Future<String> accessToken() async {
    final token = _accessToken;
    final expiresAt = _expiresAt;
    if (token == null || expiresAt == null) throw const NotSignedIn();
    if (!_now().isBefore(expiresAt.subtract(refreshMargin))) return refresh();
    return token;
  }

  /// One refresh shared by every caller. A rejection expires the session and throws
  /// [NotSignedIn]; an unreachable provider throws [AuthUnavailable] and keeps the session.
  Future<String> refresh() => _refreshing ??= _refreshOnce().whenComplete(() => _refreshing = null);

  Future<String> _refreshOnce() async {
    final generation = _generation;
    final refreshToken = _refreshToken;
    if (refreshToken == null) {
      await expire();
      throw const NotSignedIn();
    }
    final TokenSet next;
    try {
      next = await _client.refresh(refreshToken);
    } on AuthUnavailable {
      rethrow;
    } on Object {
      // Only expire the session this refresh belonged to, never a newer one.
      if (generation == _generation) await expire();
      throw const NotSignedIn();
    }
    if (generation != _generation) throw const NotSignedIn();
    await _apply(next);
    return next.accessToken;
  }

  /// Ends the session locally with the "expired" notice.
  Future<void> expire() async {
    if (state.value.status == SessionStatus.signedOut) return;
    await _wipe();
    state.value = const SessionState.signedOut(sessionExpiredNotice);
  }

  /// Revokes the refresh token, ends the IdP session, and clears local state, which happens
  /// even if either remote step fails.
  Future<void> signOut() async {
    final refreshToken = _refreshToken;
    final idToken = _idToken;
    try {
      if (refreshToken != null) await _revoke(refreshToken);
    } on Object {
      // Best effort: the refresh token still expires at the IdP.
    }
    try {
      if (idToken != null) await _client.endSession(idToken);
    } on Object {
      // Cancelled or unreachable: local sign-out still happens.
    }
    await _wipe();
    state.value = const SessionState.signedOut();
  }

  /// Saves [tokens] and signs in, unless a sign-out happened meanwhile: then the save is
  /// undone and [NotSignedIn] is thrown.
  Future<void> _apply(TokenSet tokens) async {
    final generation = _generation;
    final refreshToken = tokens.refreshToken ?? _refreshToken;
    final idToken = tokens.idToken ?? _idToken;
    if (refreshToken != null) {
      try {
        await _store.write(StoredSession(refreshToken: refreshToken, idToken: idToken));
      } on Object {
        // Storage unavailable: the session lasts until the app closes.
      }
    }
    if (generation != _generation) {
      try {
        await _store.clear();
      } on Object {
        // Nothing more to do.
      }
      throw const NotSignedIn();
    }
    _accessToken = tokens.accessToken;
    _expiresAt = tokens.expiresAt;
    _refreshToken = refreshToken;
    _idToken = idToken;
    state.value = const SessionState.signedIn();
  }

  Future<void> _wipe() async {
    _generation++;
    _accessToken = null;
    _expiresAt = null;
    _refreshToken = null;
    _idToken = null;
    try {
      await _store.clear();
    } on Object {
      // Nothing more to do; the next restore reads nothing usable either way.
    }
  }

  void dispose() => state.dispose();
}
