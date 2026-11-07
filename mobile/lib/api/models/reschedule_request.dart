// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'schedule_input.dart';

part 'reschedule_request.g.dart';

/// A new time for a job that has not started. Needs `If-Match` with the job's `ETag`.
@JsonSerializable()
class RescheduleRequest {
  const RescheduleRequest({
    required this.at,
  });
  
  factory RescheduleRequest.fromJson(Map<String, Object?> json) => _$RescheduleRequestFromJson(json);
  
  /// When to publish.
  final ScheduleInput at;

  Map<String, Object?> toJson() => _$RescheduleRequestToJson(this);
}
