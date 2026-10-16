// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'attempt_dto.g.dart';

/// One execution attempt of a job.
@JsonSerializable()
class AttemptDto {
  const AttemptDto({
    required this.attempt,
    required this.startedAt,
    required this.worker,
    this.errorCode,
    this.errorKind,
    this.errorMessage,
    this.finishedAt,
    this.outcome,
  });
  
  factory AttemptDto.fromJson(Map<String, Object?> json) => _$AttemptDtoFromJson(json);
  
  /// 1-based attempt number.
  final int attempt;

  /// Code of the error, if it failed.
  @JsonKey(name: 'error_code')
  final String? errorCode;

  /// Kind of the error, if it failed.
  @JsonKey(name: 'error_kind')
  final String? errorKind;

  /// Error message, if the job type shows it.
  @JsonKey(name: 'error_message')
  final String? errorMessage;

  /// When the attempt ended.
  @JsonKey(name: 'finished_at')
  final DateTime? finishedAt;

  /// How it ended.
  final String? outcome;

  /// When the attempt started.
  @JsonKey(name: 'started_at')
  final DateTime startedAt;

  /// The worker that ran it.
  final String worker;

  Map<String, Object?> toJson() => _$AttemptDtoToJson(this);
}
