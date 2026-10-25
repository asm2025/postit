// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

part 'probes_api.g.dart';

@RestApi()
abstract class ProbesApi {
  factory ProbesApi(Dio dio, {String? baseUrl}) = _ProbesApi;

  /// Liveness probe: answers while the process is running.
  @GET('/health')
  Future<void> health();

  /// # Errors.
  ///
  /// Returns a 503 problem naming the first failing readiness check.
  @GET('/ready')
  Future<void> ready();
}
