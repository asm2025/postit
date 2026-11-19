// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'register_push_endpoint_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RegisterPushEndpointRequest _$RegisterPushEndpointRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const [
      'auth',
      'device_label',
      'endpoint',
      'p256dh',
      'platform',
    ],
    requiredKeys: const [
      'auth',
      'device_label',
      'endpoint',
      'p256dh',
      'platform',
    ],
    disallowNullValues: const [
      'auth',
      'device_label',
      'endpoint',
      'p256dh',
      'platform',
    ],
  );
  return RegisterPushEndpointRequest(
    auth: json['auth'] as String,
    deviceLabel: json['device_label'] as String,
    endpoint: json['endpoint'] as String,
    p256dh: json['p256dh'] as String,
    platform: RegisterPushEndpointRequest._read_platform(json['platform']),
  );
}

Map<String, dynamic> _$RegisterPushEndpointRequestToJson(
  RegisterPushEndpointRequest instance,
) => <String, dynamic>{
  'auth': instance.auth,
  'device_label': instance.deviceLabel,
  'endpoint': instance.endpoint,
  'p256dh': instance.p256dh,
  'platform': ?RegisterPushEndpointRequest._write_platform(instance.platform),
};
