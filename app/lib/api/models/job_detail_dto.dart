// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'attempt_dto.dart';
import 'job_dto.dart';

part 'job_detail_dto.g.dart';

/// A job with its attempt history and retry links.
@JsonSerializable()
class JobDetailDto {
  const JobDetailDto({
    required this.attempts,
    required this.job,
    this.retriedBy,
    this.retriedFrom,
  });
  
  factory JobDetailDto.fromJson(Map<String, Object?> json) => _$JobDetailDtoFromJson(json);
  
  /// Attempts, oldest first.
  final List<AttemptDto> attempts;

  /// The job.
  final JobDto job;

  /// The job created to retry this one.
  @JsonKey(name: 'retried_by')
  final String? retriedBy;

  /// The job this one is a manual retry of.
  @JsonKey(name: 'retried_from')
  final String? retriedFrom;

  Map<String, Object?> toJson() => _$JobDetailDtoToJson(this);
}
