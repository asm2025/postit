// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'job_status.dart';

part 'publish_job_summary_dto.g.dart';

/// A publish job without its deliveries: an item of the job list.
@JsonSerializable()
class PublishJobSummaryDto {
  const PublishJobSummaryDto({
    required this.createdAt,
    required this.dryRun,
    required this.id,
    required this.nativeSchedule,
    required this.postId,
    required this.postVersion,
    required this.scheduledAt,
    required this.status,
    required this.version,
    this.cancelledAt,
    this.createdBy,
  });
  
  factory PublishJobSummaryDto.fromJson(Map<String, Object?> json) => _$PublishJobSummaryDtoFromJson(json);
  
  /// When it was cancelled.
  @JsonKey(name: 'cancelled_at')
  final DateTime? cancelledAt;

  /// When it was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Who created it (a user ID).
  @JsonKey(name: 'created_by')
  final String? createdBy;

  /// Whether it only simulates.
  @JsonKey(name: 'dry_run')
  final bool dryRun;

  /// The job.
  final String id;

  /// Whether platforms hold the post until its time.
  @JsonKey(name: 'native_schedule')
  final bool nativeSchedule;

  /// The post it publishes.
  @JsonKey(name: 'post_id')
  final String postId;

  /// The post version it froze.
  @JsonKey(name: 'post_version')
  final int postVersion;

  /// When its deliveries become due.
  @JsonKey(name: 'scheduled_at')
  final DateTime scheduledAt;

  /// Where it is, derived from its deliveries.
  final JobStatus status;

  /// Bumped on every delivery change; the `ETag` is `"v{version}"`.
  final int version;

  Map<String, Object?> toJson() => _$PublishJobSummaryDtoToJson(this);
}
