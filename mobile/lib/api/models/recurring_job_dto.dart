// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'recurring_job_dto.g.dart';

/// A recurring job and its schedule.
@JsonSerializable()
class RecurringJobDto {
  const RecurringJobDto({
    required this.name,
    required this.schedule,
    this.lastOutcome,
    this.lastRunAt,
    this.nextRunAt,
  });
  
  factory RecurringJobDto.fromJson(Map<String, Object?> json) => _$RecurringJobDtoFromJson(json);
  
  /// How the newest run ended.
  @JsonKey(name: 'last_outcome')
  final String? lastOutcome;

  /// When the newest run was scheduled.
  @JsonKey(name: 'last_run_at')
  final DateTime? lastRunAt;

  /// Job type name.
  final String name;

  /// When the next run is due.
  @JsonKey(name: 'next_run_at')
  final DateTime? nextRunAt;

  /// Cron text of the schedule.
  final String schedule;

  Map<String, Object?> toJson() => _$RecurringJobDtoToJson(this);
}
