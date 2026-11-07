// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'schedule_input.dart';

part 'publish_request.g.dart';

/// What to publish a saved post as. Needs `If-Match` with the post's `ETag`.
@JsonSerializable()
class PublishRequest {
  const PublishRequest({
    required this.targets,
    this.at,
    this.confirm,
    this.dryRun,
    this.nativeSchedule,
  });
  
  factory PublishRequest.fromJson(Map<String, Object?> json) => _$PublishRequestFromJson(json);
  
  /// When to publish; now when absent.
  final ScheduleInput? at;

  /// The caller's confirmation of a live publish.
  final bool? confirm;

  /// Whether the job only simulates.
  @JsonKey(name: 'dry_run')
  final bool? dryRun;

  /// Whether platforms that can hold the post until `at` do so.
  @JsonKey(name: 'native_schedule')
  final bool? nativeSchedule;

  /// The accounts to publish to; a client expands target groups itself.
  final List<String> targets;

  Map<String, Object?> toJson() => _$PublishRequestToJson(this);
}
