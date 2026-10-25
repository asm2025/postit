// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'job_count_dto.dart';
import 'throughput_bucket_dto.dart';

part 'job_stats_dto.g.dart';

/// Dashboard aggregates.
@JsonSerializable()
class JobStatsDto {
  const JobStatsDto({
    required this.queues,
    required this.states,
    required this.throughput,
    required this.types,
    required this.windowHours,
  });
  
  factory JobStatsDto.fromJson(Map<String, Object?> json) => _$JobStatsDtoFromJson(json);
  
  /// Due jobs per queue.
  final List<JobCountDto> queues;

  /// Jobs per state.
  final List<JobCountDto> states;

  /// Attempt outcomes over the window.
  final List<ThroughputBucketDto> throughput;

  /// Jobs per type.
  final List<JobCountDto> types;

  /// Length of the throughput window in hours.
  @JsonKey(name: 'window_hours')
  final int windowHours;

  Map<String, Object?> toJson() => _$JobStatsDtoToJson(this);
}
