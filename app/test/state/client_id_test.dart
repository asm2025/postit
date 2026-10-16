import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

void main() {
  final config = AuthConfigDto.fromJson({
    'issuer': 'https://idp.test',
    'client_id': 'web',
    'android_client_id': 'android',
    'ios_client_id': null,
    'scopes': ['openid'],
  });

  test('each platform reads its own client id, never the web one', () {
    expect(clientIdFor(config, MobilePlatform.android), 'android');
    expect(clientIdFor(config, MobilePlatform.ios), isNull);
  });
}
