// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'delivery_dto.dart';
import 'job_status.dart';

part 'publish_job_dto.g.dart';

/// A publish job with the deliveries the caller can see.
@JsonSerializable()
class PublishJobDto {
  const PublishJobDto({
    required this.createdAt,
    required this.dryRun,
    required this.id,
    required this.nativeSchedule,
    required this.postId,
    required this.postVersion,
    required this.scheduledAt,
    required this.status,
    required this.version,
    required this.deliveries,
    this.cancelledAt,
    this.createdBy,
  });
  
  factory PublishJobDto.fromJson(Map<String, Object?> json) => _$PublishJobDtoFromJson(json);
  
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

  /// Its deliveries inside the caller's account limit.
  final List<DeliveryDto> deliveries;

  Map<String, Object?> toJson() => _$PublishJobDtoToJson(this);
}
