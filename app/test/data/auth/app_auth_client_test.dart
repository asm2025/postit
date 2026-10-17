import 'package:flutter/services.dart';
import 'package:flutter_appauth/flutter_appauth.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/app_auth_client.dart';
import 'package:postit/data/auth/auth_client.dart';

class _ThrowingAppAuth extends Fake implements FlutterAppAuth {
  _ThrowingAppAuth(this.error);
  final Object error;

  @override
  Future<TokenResponse> token(TokenRequest request) async => throw error;
}

void main() {
  const config = OidcClientConfig(
    issuer: 'https://idp.test',
    clientId: 'client',
    scopes: ['openid'],
    redirectUrl: 'app://auth/callback',
    postLogoutRedirectUrl: 'app://auth/logout',
  );

  Future<TokenSet> refreshThrowing(Object error) =>
      AppAuthClient(config, appAuth: _ThrowingAppAuth(error)).refresh('refresh');

  FlutterAppAuthPlatformException appAuthError(String? error) => FlutterAppAuthPlatformException(
    code: 'token_failed',
    platformErrorDetails: FlutterAppAuthPlatformErrorDetails(error: error, errorDescription: 'described'),
  );

  test('an OAuth error from the provider is a rejection', () async {
    await expectLater(refreshThrowing(appAuthError('invalid_grant')), throwsA(isA<AuthFailed>()));
  });

  test('an AppAuth error without an OAuth code means the provider was not reached', () async {
    await expectLater(refreshThrowing(appAuthError(null)), throwsA(isA<AuthUnavailable>()));
  });

  test('a plugin failure that is not a PlatformException never rejects the session', () async {
    await expectLater(refreshThrowing(MissingPluginException('no AppAuth')), throwsA(isA<AuthUnavailable>()));
    await expectLater(refreshThrowing(StateError('plugin crashed')), throwsA(isA<AuthUnavailable>()));
  });
}
