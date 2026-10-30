// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'register_push_endpoint_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RegisterPushEndpointRequest _$RegisterPushEndpointRequestFromJson(
  Map<String, dynamic> json,
) => RegisterPushEndpointRequest(
  auth: json['auth'] as String,
  deviceLabel: json['device_label'] as String,
  endpoint: json['endpoint'] as String,
  p256dh: json['p256dh'] as String,
  platform: PushPlatformDto.fromJson(json['platform'] as String),
);

Map<String, dynamic> _$RegisterPushEndpointRequestToJson(
  RegisterPushEndpointRequest instance,
) => <String, dynamic>{
  'auth': instance.auth,
  'device_label': instance.deviceLabel,
  'endpoint': instance.endpoint,
  'p256dh': instance.p256dh,
  'platform': _$PushPlatformDtoEnumMap[instance.platform]!,
};

const _$PushPlatformDtoEnumMap = {
  PushPlatformDto.android: 'android',
  PushPlatformDto.ios: 'ios',
  PushPlatformDto.web: 'web',
  PushPlatformDto.$unknown: r'$unknown',
};
