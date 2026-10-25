import 'dart:io';

import 'package:dio/dio.dart';
import 'package:dio/io.dart';

/// A dio for the postit API (or the IdP, for absolute URLs). problem+json decodes as JSON.
/// [trust] replaces the system roots for TLS (see `devTrust`); null keeps the default client.
/// [acceptLanguage] returns the language the UI shows now; it is sent as `Accept-Language` on every request.
Dio createDio(Uri baseUrl, {SecurityContext? trust, String Function()? acceptLanguage}) {
  final dio = Dio(
    BaseOptions(
      baseUrl: baseUrl.toString(),
      connectTimeout: const Duration(seconds: 10),
      receiveTimeout: const Duration(seconds: 30),
      headers: {'Accept': 'application/json'},
    ),
  );
  if (acceptLanguage != null) {
    dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) {
          options.headers['Accept-Language'] = acceptLanguage();
          handler.next(options);
        },
      ),
    );
  }
  if (trust != null) {
    dio.httpClientAdapter = IOHttpClientAdapter(createHttpClient: () => HttpClient(context: trust));
  }
  return dio;
}
