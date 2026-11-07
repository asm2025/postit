// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/agent_descriptor.dart';

part 'platforms_api.g.dart';

@RestApi()
abstract class PlatformsApi {
  factory PlatformsApi(Dio dio, {String? baseUrl}) = _PlatformsApi;

  /// The platforms accounts can be connected to here: compiled into this build, configured, and.
  /// switched on, in the order the connect screen shows them. The icon is left out; fetch it from.
  /// `GET /platforms/{id}/icon`. Any signed-in user may read it, and it is the same through.
  /// `X-Postit-Act-As`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  @GET('/api/v1/platforms')
  Future<List<AgentDescriptor>> listPlatforms();

  /// The platform's icon as a PNG, cached for a day and tagged with a hash of its bytes. A.
  /// platform that `GET /platforms` does not list answers 404.
  ///
  /// # Errors.
  ///
  /// 404 for a platform that is unknown, not compiled in, not configured, or switched off.
  ///
  /// [id] - Platform ID.
  @GET('/api/v1/platforms/{id}/icon')
  Future<List<int>> getPlatformIcon({
    @Path('id') required String id,
  });
}
