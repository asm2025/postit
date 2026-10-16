// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_stats_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobStatsDto _$JobStatsDtoFromJson(Map<String, dynamic> json) => JobStatsDto(
  queues: (json['queues'] as List<dynamic>)
      .map((e) => JobCountDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  states: (json['states'] as List<dynamic>)
      .map((e) => JobCountDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  throughput: (json['throughput'] as List<dynamic>)
      .map((e) => ThroughputBucketDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  types: (json['types'] as List<dynamic>)
      .map((e) => JobCountDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  windowHours: (json['window_hours'] as num).toInt(),
);

Map<String, dynamic> _$JobStatsDtoToJson(JobStatsDto instance) =>
    <String, dynamic>{
      'queues': instance.queues,
      'states': instance.states,
      'throughput': instance.throughput,
      'types': instance.types,
      'window_hours': instance.windowHours,
    };
