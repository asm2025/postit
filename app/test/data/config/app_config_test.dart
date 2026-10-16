import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/config/app_config.dart';

void main() {
  test('parses the dart-define values and trims a trailing slash', () {
    final c = AppConfig.parse(apiBaseUrl: 'https://postit.local:44310/', env: 'development');
    expect(c.apiBaseUrl.toString(), 'https://postit.local:44310');
    expect(c.env, AppEnv.development);
    expect(c.redirectUrl, 'postit-dev://auth/callback');
    expect(c.postLogoutRedirectUrl, 'postit-dev://auth/logout');
  });

  test('each environment has its own scheme', () {
    expect(AppConfig.parse(apiBaseUrl: 'https://a', env: 'qa').scheme, 'postit-qa');
    expect(AppConfig.parse(apiBaseUrl: 'https://a', env: 'production').scheme, 'postit');
  });

  test('a missing base URL names the fix', () {
    expect(
      () => AppConfig.parse(apiBaseUrl: '', env: 'qa'),
      throwsA(isA<FormatException>().having((e) => e.message, 'message', contains('--dart-define-from-file'))),
    );
  });

  test('an unknown environment is rejected', () {
    expect(() => AppConfig.parse(apiBaseUrl: 'https://a', env: 'staging'), throwsFormatException);
  });
}
