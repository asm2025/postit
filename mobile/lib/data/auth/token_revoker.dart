import 'package:dio/dio.dart';
import 'package:postit/data/auth/auth_client.dart';

/// Revokes a refresh token at the provider's `revocation_endpoint` (RFC 7009).
class TokenRevoker {
  TokenRevoker(this._dio, this._config);

  final Dio _dio;
  final OidcClientConfig _config;

  Future<void> revoke(String refreshToken) async {
    final discovery = await _dio.get<Map<String, dynamic>>(_config.discoveryUrl);
    final endpoint = discovery.data?['revocation_endpoint'];
    // A provider without revocation leaves the token to expire on its own.
    if (endpoint is! String) return;
    await _dio.post<void>(
      endpoint,
      data: {'token': refreshToken, 'token_type_hint': 'refresh_token', 'client_id': _config.clientId},
      options: Options(contentType: Headers.formUrlEncodedContentType),
    );
  }
}
