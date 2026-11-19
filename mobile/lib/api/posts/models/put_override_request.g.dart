// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_override_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutOverrideRequest _$PutOverrideRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['options', 'patch'],
    disallowNullValues: const ['options', 'patch'],
  );
  return PutOverrideRequest(
    options: json['options'] == null
        ? null
        : PutOverrideRequestOptions.fromJson(
            json['options'] as Map<String, dynamic>,
          ),
    patch: json['patch'] == null
        ? null
        : PostOverride.fromJson(json['patch'] as Map<String, dynamic>),
  );
}

Map<String, dynamic> _$PutOverrideRequestToJson(PutOverrideRequest instance) =>
    <String, dynamic>{
      'options': ?instance.options?.toJson(),
      'patch': ?instance.patch?.toJson(),
    };
