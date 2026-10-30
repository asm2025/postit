// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'push_platform_dto.dart';

part 'register_push_endpoint_request.g.dart';

/// A device to receive push notifications, from the browser's or app's push subscription.
/// Registering an endpoint again replaces its keys, platform, and label and keeps its ID.
@JsonSerializable()
class RegisterPushEndpointRequest {
  const RegisterPushEndpointRequest({
    required this.auth,
    required this.deviceLabel,
    required this.endpoint,
    required this.p256dh,
    required this.platform,
  });
  
  factory RegisterPushEndpointRequest.fromJson(Map<String, Object?> json) => _$RegisterPushEndpointRequestFromJson(json);
  
  /// The subscription's 16-byte authentication secret, base64url.
  final String auth;

  /// A name for the device, shown in the profile; at most 100 characters.
  @JsonKey(name: 'device_label')
  final String deviceLabel;

  /// The push service URL: `https`, or `http` only when the deployment allows private.
  /// endpoints (a self-hosted `UnifiedPush` distributor on a LAN). At most 2048 characters.
  final String endpoint;

  /// The subscription's public key: a 65-byte uncompressed P-256 point, base64url.
  final String p256dh;

  /// The push service.
  final PushPlatformDto platform;

  Map<String, Object?> toJson() => _$RegisterPushEndpointRequestToJson(this);
}
