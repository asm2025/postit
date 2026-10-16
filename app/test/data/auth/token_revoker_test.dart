import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/token_revoker.dart';

import '../../helpers/scripted_adapter.dart';

const _config = OidcClientConfig(
  issuer: 'https://idp.test',
  clientId: 'android-1',
  scopes: ['openid'],
  redirectUrl: 'postit-dev://auth/callback',
  postLogoutRedirectUrl: 'postit-dev://auth/logout',
);

void main() {
  test('posts the refresh token to the revocation endpoint', () async {
    final adapter = ScriptedAdapter(
      (o) async => o.path.endsWith('openid-configuration')
          ? ScriptedAdapter.json(200, {'revocation_endpoint': 'https://idp.test/oauth/v2/revoke'})
          : ScriptedAdapter.json(200, {}),
    );
    final dio = Dio()..httpClientAdapter = adapter;
    await TokenRevoker(dio, _config).revoke('rt-1');
    final post = adapter.requests.last;
    expect(post.method, 'POST');
    expect(post.uri.toString(), 'https://idp.test/oauth/v2/revoke');
    expect(post.data, {'token': 'rt-1', 'token_type_hint': 'refresh_token', 'client_id': 'android-1'});
    expect(post.contentType, Headers.formUrlEncodedContentType);
  });

  test('a provider without revocation is skipped', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {'issuer': 'https://idp.test'}));
    final dio = Dio()..httpClientAdapter = adapter;
    await TokenRevoker(dio, _config).revoke('rt-1');
    expect(adapter.requests, hasLength(1));
  });
}
