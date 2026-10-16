export 'package:postit/data/auth/not_signed_in.dart' show AuthUnavailable;

/// Tokens from one sign-in or refresh.
class TokenSet {
  const TokenSet({required this.accessToken, required this.expiresAt, this.refreshToken, this.idToken});

  final String accessToken;
  final DateTime expiresAt;
  final String? refreshToken;
  final String? idToken;
}

/// The OIDC client this build signs in with.
class OidcClientConfig {
  const OidcClientConfig({
    required this.issuer,
    required this.clientId,
    required this.scopes,
    required this.redirectUrl,
    required this.postLogoutRedirectUrl,
  });

  final String issuer;
  final String clientId;
  final List<String> scopes;
  final String redirectUrl;
  final String postLogoutRedirectUrl;

  String get discoveryUrl => '$issuer/.well-known/openid-configuration';
}

/// The user closed the browser before finishing.
class AuthCancelled implements Exception {
  const AuthCancelled();
}

/// The identity provider refused or failed; [message] is safe to show.
class AuthFailed implements Exception {
  const AuthFailed(this.message);
  final String message;

  @override
  String toString() => 'AuthFailed: $message';
}

/// The only door to the OIDC library, so tests use a fake and a library change touches
/// one file. Every method throws [AuthCancelled], [AuthFailed] (rejected), or
/// [AuthUnavailable] (not reached).
abstract interface class AuthClient {
  Future<TokenSet> signIn();
  Future<TokenSet> refresh(String refreshToken);
  Future<void> endSession(String idTokenHint);
}
