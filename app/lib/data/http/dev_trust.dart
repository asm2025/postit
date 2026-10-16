import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:postit/data/config/app_config.dart';

/// Bundled only in the development flavor (pubspec `flavors:`), copied in by Gradle.
const devCaAsset = 'assets/dev/postit-dev-ca.crt';

/// Dart's HttpClient ignores Android's network_security_config, so debug development builds
/// trust the postit dev CA here, on top of the system roots. Null everywhere else, and when
/// the asset is missing or unreadable, which means the system roots only.
Future<SecurityContext?> devTrust(AppConfig config, {AssetBundle? bundle}) async {
  if (!kDebugMode || config.env != AppEnv.development) return null;
  try {
    final pem = await (bundle ?? rootBundle).load(devCaAsset);
    return SecurityContext(withTrustedRoots: true)
      ..setTrustedCertificatesBytes(pem.buffer.asUint8List(pem.offsetInBytes, pem.lengthInBytes));
  } on Object {
    return null;
  }
}
