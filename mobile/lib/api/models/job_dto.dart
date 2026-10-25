// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'job_actions_dto.dart';
import 'job_state_dto.dart';

part 'job_dto.g.dart';

/// One job.
@JsonSerializable()
class JobDto {
  const JobDto({
    required this.actions,
    required this.attempts,
    required this.createdAt,
    required this.id,
    required this.jobType,
    required this.queue,
    required this.runAt,
    required this.state,
    required this.summary,
    this.actionsDisabledReason,
    this.errorCode,
    this.errorKind,
    this.finishedAt,
    this.maxAttempts,
    this.recurringName,
    this.startedAt,
  });
  
  factory JobDto.fromJson(Map<String, Object?> json) => _$JobDtoFromJson(json);
  
  /// Which actions apply.
  final JobActionsDto actions;

  /// Why no action applies, when none does.
  @JsonKey(name: 'actions_disabled_reason')
  final String? actionsDisabledReason;

  /// Attempts made so far.
  final int attempts;

  /// When the job was enqueued.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Code of the last error.
  @JsonKey(name: 'error_code')
  final String? errorCode;

  /// Kind of the last error.
  @JsonKey(name: 'error_kind')
  final String? errorKind;

  /// When the job reached a final state.
  @JsonKey(name: 'finished_at')
  final DateTime? finishedAt;

  /// Job ID.
  final String id;

  /// Job type name.
  @JsonKey(name: 'job_type')
  final String jobType;

  /// Attempts allowed in total.
  @JsonKey(name: 'max_attempts')
  final int? maxAttempts;

  /// Queue the job runs on.
  final String queue;

  /// Name of the recurring schedule that enqueued it.
  @JsonKey(name: 'recurring_name')
  final String? recurringName;

  /// When the job is (or was) due.
  @JsonKey(name: 'run_at')
  final DateTime runAt;

  /// When the current or last attempt started.
  @JsonKey(name: 'started_at')
  final DateTime? startedAt;

  /// Current state.
  final JobStateDto state;

  /// Non-sensitive fields of the payload.
  final Map<String, String> summary;

  Map<String, Object?> toJson() => _$JobDtoToJson(this);
}
