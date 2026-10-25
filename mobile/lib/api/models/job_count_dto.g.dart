// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_count_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobCountDto _$JobCountDtoFromJson(Map<String, dynamic> json) => JobCountDto(
  count: (json['count'] as num).toInt(),
  key: json['key'] as String,
);

Map<String, dynamic> _$JobCountDtoToJson(JobCountDto instance) =>
    <String, dynamic>{'count': instance.count, 'key': instance.key};
