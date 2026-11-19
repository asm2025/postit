// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publish_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishRequest _$PublishRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'at',
      'confirm',
      'dry_run',
      'native_schedule',
      'targets',
    ],
    requiredKeys: const ['targets'],
    disallowNullValues: const [
      'confirm',
      'dry_run',
      'native_schedule',
      'targets',
    ],
  );
  return PublishRequest(
    at: !json.containsKey('at') ? null : PublishRequest._read_at(json['at']),
    confirm: json['confirm'] as bool?,
    dryRun: json['dry_run'] as bool?,
    nativeSchedule: json['native_schedule'] as bool?,
    targets: (json['targets'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
  );
}

Map<String, dynamic> _$PublishRequestToJson(
  PublishRequest instance,
) => <String, dynamic>{
  if (instance.at case final $value?) 'at': PublishRequest._write_at($value),
  'confirm': ?instance.confirm,
  'dry_run': ?instance.dryRun,
  'native_schedule': ?instance.nativeSchedule,
  'targets': instance.targets,
};
