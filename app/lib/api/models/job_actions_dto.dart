// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'job_actions_dto.g.dart';

/// The console actions the job currently offers.
@JsonSerializable()
class JobActionsDto {
  const JobActionsDto({
    required this.cancel,
    required this.delete,
    required this.retry,
  });
  
  factory JobActionsDto.fromJson(Map<String, Object?> json) => _$JobActionsDtoFromJson(json);
  
  /// Cancel the job before it runs.
  final bool cancel;

  /// Delete the job and its history.
  final bool delete;

  /// Re-run the job as a new job.
  final bool retry;

  Map<String, Object?> toJson() => _$JobActionsDtoToJson(this);
}
