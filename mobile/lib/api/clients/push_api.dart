// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/push_endpoint_created.dart';
import '../models/register_push_endpoint_request.dart';
import '../models/vapid_key_dto.dart';

part 'push_api.g.dart';

@RestApi()
abstract class PushApi {
  factory PushApi(Dio dio, {String? baseUrl}) = _PushApi;

  /// Registers one of the caller's devices for push notifications. The same endpoint again.
  /// replaces its keys, platform, and label, and answers with the same ID.
  ///
  /// # Errors.
  ///
  /// 404 `push_disabled` when push is off; 422 `push_endpoint_invalid` (with `field`) for a.
  /// malformed endpoint or key, `validation_failed` for a label over 100 characters.
  @POST('/api/v1/me/push-endpoints')
  Future<PushEndpointCreated> registerPushEndpoint({
    @Body() required RegisterPushEndpointRequest body,
  });

  /// Removes one of the caller's devices. Works with push off.
  ///
  /// # Errors.
  ///
  /// 404 `not_found` when the caller has no such device.
  ///
  /// [id] - Device ID.
  @DELETE('/api/v1/me/push-endpoints/{id}')
  Future<void> deletePushEndpoint({
    @Path('id') required String id,
  });

  /// The deployment's VAPID public key, which browsers subscribe with.
  ///
  /// # Errors.
  ///
  /// 404 `push_disabled` when push is off.
  @GET('/api/v1/push/vapid-public-key')
  Future<VapidKeyDto> getVapidPublicKey();
}
