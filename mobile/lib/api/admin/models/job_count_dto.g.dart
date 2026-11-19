// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_count_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobCountDto _$JobCountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['count', 'key'],
    disallowNullValues: const ['count', 'key'],
  );
  return JobCountDto(
    count: JobCountDto._read_count(json['count']),
    key: json['key'] as String,
    additionalProperties:
        JobCountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : JobCountDto._readExtras(
            JobCountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobCountDtoToJson(JobCountDto instance) =>
    <String, dynamic>{
      'count': ?JobCountDto._write_count(instance.count),
      'key': instance.key,
    };
