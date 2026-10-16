import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// What survives an app restart: never the access token.
class StoredSession {
  const StoredSession({required this.refreshToken, this.idToken});
  final String refreshToken;
  final String? idToken;
}

abstract interface class SessionStore {
  Future<StoredSession?> read();
  Future<void> write(StoredSession session);
  Future<void> clear();
}

/// Keystore (Android) or Keychain (iOS).
class SecureSessionStore implements SessionStore {
  SecureSessionStore([this._storage = const FlutterSecureStorage()]);

  final FlutterSecureStorage _storage;
  static const _refreshKey = 'postit.refresh_token';
  static const _idKey = 'postit.id_token';

  @override
  Future<StoredSession?> read() async {
    final refresh = await _storage.read(key: _refreshKey);
    if (refresh == null) return null;
    return StoredSession(
      refreshToken: refresh,
      idToken: await _storage.read(key: _idKey),
    );
  }

  @override
  Future<void> write(StoredSession session) async {
    await _storage.write(key: _refreshKey, value: session.refreshToken);
    final id = session.idToken;
    if (id == null) {
      await _storage.delete(key: _idKey);
    } else {
      await _storage.write(key: _idKey, value: id);
    }
  }

  @override
  Future<void> clear() async {
    await _storage.delete(key: _refreshKey);
    await _storage.delete(key: _idKey);
  }
}
