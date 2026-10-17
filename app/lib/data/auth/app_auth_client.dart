import 'package:flutter/services.dart';
import 'package:flutter_appauth/flutter_appauth.dart';
import 'package:postit/data/auth/auth_client.dart';

/// OAuth error codes (RFC 6749 §4.1.2.1 and §5.2, OIDC Core §3.1.2.6): the provider answered
/// and said no. AppAuth leaves `error` null for its own failures (network, discovery, TLS).
/// `server_error` and `temporarily_unavailable` are left out on purpose: they say nothing
/// about the refresh token, so they keep the session like a network failure.
const _oauthErrors = {
  'invalid_request',
  'invalid_client',
  'invalid_grant',
  'unauthorized_client',
  'unsupported_grant_type',
  'invalid_scope',
  'access_denied',
  'unsupported_response_type',
  'interaction_required',
  'login_required',
  'account_selection_required',
  'consent_required',
};

/// True when the provider rejected the request, as opposed to not being reached. Task 5's
/// spike confirms this rule; adjust it there if the recorded error details differ.
bool _isRejection(FlutterAppAuthPlatformErrorDetails details) => _oauthErrors.contains(details.error);

/// [AuthClient] over `flutter_appauth`: PKCE in the system browser.
class AppAuthClient implements AuthClient {
  AppAuthClient(this._config, {this._appAuth = const FlutterAppAuth()});

  final OidcClientConfig _config;
  final FlutterAppAuth _appAuth;

  @override
  Future<TokenSet> signIn() => _guard(
    () async => _tokens(
      await _appAuth.authorizeAndExchangeCode(
        AuthorizationTokenRequest(
          _config.clientId,
          _config.redirectUrl,
          discoveryUrl: _config.discoveryUrl,
          scopes: _config.scopes,
        ),
      ),
    ),
  );

  @override
  Future<TokenSet> refresh(String refreshToken) => _guard(
    () async => _tokens(
      await _appAuth.token(
        TokenRequest(
          _config.clientId,
          _config.redirectUrl,
          discoveryUrl: _config.discoveryUrl,
          refreshToken: refreshToken,
          scopes: _config.scopes,
        ),
      ),
    ),
  );

  @override
  Future<void> endSession(String idTokenHint) => _guard(() async {
    await _appAuth.endSession(
      EndSessionRequest(
        idTokenHint: idTokenHint,
        postLogoutRedirectUrl: _config.postLogoutRedirectUrl,
        discoveryUrl: _config.discoveryUrl,
      ),
    );
  });

  Future<T> _guard<T>(Future<T> Function() call) async {
    try {
      return await call();
    } on FlutterAppAuthUserCancelledException {
      throw const AuthCancelled();
    } on FlutterAppAuthPlatformException catch (e) {
      final details = e.platformErrorDetails;
      if (!_isRejection(details)) throw const AuthUnavailable();
      throw AuthFailed(details.errorDescription ?? details.error ?? 'Sign-in failed.');
    } on AuthFailed {
      rethrow;
    } on Object {
      // Any other plugin failure (a PlatformException, a missing plugin, a crash): treat as
      // not reached, never as a rejected session, so the stored session survives.
      throw const AuthUnavailable();
    }
  }

  TokenSet _tokens(TokenResponse response) {
    final access = response.accessToken;
    if (access == null) throw const AuthFailed('The identity provider returned no access token.');
    return TokenSet(
      accessToken: access,
      expiresAt: response.accessTokenExpirationDateTime ?? DateTime.now().add(const Duration(minutes: 5)),
      refreshToken: response.refreshToken,
      idToken: response.idToken,
    );
  }
}
