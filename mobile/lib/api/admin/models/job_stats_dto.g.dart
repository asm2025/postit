// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_stats_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobStatsDto _$JobStatsDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'queues',
      'states',
      'throughput',
      'types',
      'window_hours',
    ],
    disallowNullValues: const [
      'queues',
      'states',
      'throughput',
      'types',
      'window_hours',
    ],
  );
  return JobStatsDto(
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
    windowHours: JobStatsDto._read_windowHours(json['window_hours']),
    additionalProperties:
        JobStatsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : JobStatsDto._readExtras(
            JobStatsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobStatsDtoToJson(JobStatsDto instance) =>
    <String, dynamic>{
      'queues': instance.queues.map((e) => e.toJson()).toList(),
      'states': instance.states.map((e) => e.toJson()).toList(),
      'throughput': instance.throughput.map((e) => e.toJson()).toList(),
      'types': instance.types.map((e) => e.toJson()).toList(),
      'window_hours': ?JobStatsDto._write_windowHours(instance.windowHours),
    };
