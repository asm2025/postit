// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validate_post_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidatePostRequest _$ValidatePostRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['at', 'native_schedule', 'targets'],
    disallowNullValues: const ['native_schedule'],
  );
  return ValidatePostRequest(
    at: !json.containsKey('at')
        ? null
        : ValidatePostRequest._read_at(json['at']),
    nativeSchedule: json['native_schedule'] as bool?,
    targets: (json['targets'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
  );
}

Map<String, dynamic> _$ValidatePostRequestToJson(
  ValidatePostRequest instance,
) => <String, dynamic>{
  if (instance.at case final $value?)
    'at': ValidatePostRequest._write_at($value),
  'native_schedule': ?instance.nativeSchedule,
  'targets': ?instance.targets,
};
