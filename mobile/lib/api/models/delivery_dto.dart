// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'delivery_state_dto.dart';
import 'direct_counts_dto.dart';
import 'last_error_dto.dart';
import 'operation_dto.dart';
import 'park_reason.dart';

part 'delivery_dto.g.dart';

/// One post going to one account.
@JsonSerializable()
class DeliveryDto {
  const DeliveryDto({
    required this.accountId,
    required this.attemptCount,
    required this.id,
    required this.jobId,
    required this.operation,
    required this.postId,
    required this.state,
    this.createdBy,
    this.direct,
    this.lastError,
    this.nextAttemptAt,
    this.parkedReason,
    this.remoteId,
    this.remotePublishAt,
    this.remoteUrl,
  });
  
  factory DeliveryDto.fromJson(Map<String, Object?> json) => _$DeliveryDtoFromJson(json);
  
  /// The account it goes to.
  @JsonKey(name: 'account_id')
  final String accountId;

  /// Attempts counted so far.
  @JsonKey(name: 'attempt_count')
  final int attemptCount;

  /// Who created the job it belongs to (a user ID).
  @JsonKey(name: 'created_by')
  final String? createdBy;

  /// Progress of a direct send.
  final DirectCountsDto? direct;

  /// The delivery.
  final String id;

  /// The job it belongs to.
  @JsonKey(name: 'job_id')
  final String jobId;

  /// The classified error behind the last failure or park.
  @JsonKey(name: 'last_error')
  final LastErrorDto? lastError;

  /// When it is next due.
  @JsonKey(name: 'next_attempt_at')
  final DateTime? nextAttemptAt;

  /// What it does on the platform.
  final OperationDto operation;

  /// Why it is parked, while `state` is `parked`.
  @JsonKey(name: 'parked_reason')
  final ParkReason? parkedReason;

  /// The post it publishes.
  @JsonKey(name: 'post_id')
  final String postId;

  /// The platform's ID for the post.
  @JsonKey(name: 'remote_id')
  final String? remoteId;

  /// When a natively scheduled post goes live.
  @JsonKey(name: 'remote_publish_at')
  final DateTime? remotePublishAt;

  /// The post's public URL.
  @JsonKey(name: 'remote_url')
  final String? remoteUrl;

  /// Where it is.
  final DeliveryStateDto state;

  Map<String, Object?> toJson() => _$DeliveryDtoToJson(this);
}
