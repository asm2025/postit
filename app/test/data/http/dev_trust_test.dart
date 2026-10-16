import 'dart:io';

import 'package:dio/io.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/dev_trust.dart';
import 'package:postit/data/http/dio_factory.dart';

class _Bundle extends CachingAssetBundle {
  _Bundle(this.assets);
  final Map<String, List<int>> assets;

  @override
  Future<ByteData> load(String key) async {
    final bytes = assets[key];
    if (bytes == null) throw FlutterError('missing $key');
    return ByteData.sublistView(Uint8List.fromList(bytes));
  }
}

void main() {
  final dev = AppConfig.parse(apiBaseUrl: 'https://postit.local:44310', env: 'development');

  test('qa and production never load the dev CA', () async {
    for (final env in ['qa', 'production']) {
      final config = AppConfig.parse(apiBaseUrl: 'https://api.test', env: env);
      expect(
        await devTrust(
          config,
          bundle: _Bundle({
            devCaAsset: [1, 2, 3],
          }),
        ),
        isNull,
        reason: env,
      );
    }
  });

  test('a development build without the asset falls back to the system roots', () async {
    expect(await devTrust(dev, bundle: _Bundle({})), isNull);
  });

  test('an unreadable certificate falls back to the system roots', () async {
    expect(await devTrust(dev, bundle: _Bundle({devCaAsset: 'not a certificate'.codeUnits})), isNull);
  });

  test('createDio uses the given trust and the default adapter otherwise', () {
    expect(
      createDio(Uri.parse('https://api.test'), trust: SecurityContext()).httpClientAdapter,
      isA<IOHttpClientAdapter>().having((a) => a.createHttpClient, 'createHttpClient', isNotNull),
    );
    expect(
      createDio(Uri.parse('https://api.test')).httpClientAdapter,
      isA<IOHttpClientAdapter>().having((a) => a.createHttpClient, 'createHttpClient', isNull),
    );
  });
}
