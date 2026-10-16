// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'throughput_bucket_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ThroughputBucketDto _$ThroughputBucketDtoFromJson(Map<String, dynamic> json) =>
    ThroughputBucketDto(
      failed: (json['failed'] as num).toInt(),
      start: DateTime.parse(json['start'] as String),
      succeeded: (json['succeeded'] as num).toInt(),
    );

Map<String, dynamic> _$ThroughputBucketDtoToJson(
  ThroughputBucketDto instance,
) => <String, dynamic>{
  'failed': instance.failed,
  'start': instance.start.toIso8601String(),
  'succeeded': instance.succeeded,
};
