// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'job_action_response.g.dart';

/// The job a retry or trigger created.
@JsonSerializable()
class JobActionResponse {
  const JobActionResponse({
    required this.jobId,
  });
  
  factory JobActionResponse.fromJson(Map<String, Object?> json) => _$JobActionResponseFromJson(json);
  
  /// The new job's ID.
  @JsonKey(name: 'job_id')
  final String jobId;

  Map<String, Object?> toJson() => _$JobActionResponseToJson(this);
}
