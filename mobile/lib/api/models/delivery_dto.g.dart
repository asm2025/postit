// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delivery_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeliveryDto _$DeliveryDtoFromJson(Map<String, dynamic> json) => DeliveryDto(
  accountId: json['account_id'] as String,
  attemptCount: (json['attempt_count'] as num).toInt(),
  id: json['id'] as String,
  jobId: json['job_id'] as String,
  operation: OperationDto.fromJson(json['operation'] as String),
  postId: json['post_id'] as String,
  state: DeliveryStateDto.fromJson(json['state'] as String),
  createdBy: json['created_by'] as String?,
  direct: json['direct'] == null
      ? null
      : DirectCountsDto.fromJson(json['direct'] as Map<String, dynamic>),
  lastError: json['last_error'] == null
      ? null
      : LastErrorDto.fromJson(json['last_error'] as Map<String, dynamic>),
  nextAttemptAt: json['next_attempt_at'] == null
      ? null
      : DateTime.parse(json['next_attempt_at'] as String),
  parkedReason: json['parked_reason'] == null
      ? null
      : ParkReason.fromJson(json['parked_reason'] as String),
  remoteId: json['remote_id'] as String?,
  remotePublishAt: json['remote_publish_at'] == null
      ? null
      : DateTime.parse(json['remote_publish_at'] as String),
  remoteUrl: json['remote_url'] as String?,
);

Map<String, dynamic> _$DeliveryDtoToJson(DeliveryDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'attempt_count': instance.attemptCount,
      'created_by': instance.createdBy,
      'direct': instance.direct,
      'id': instance.id,
      'job_id': instance.jobId,
      'last_error': instance.lastError,
      'next_attempt_at': instance.nextAttemptAt?.toIso8601String(),
      'operation': _$OperationDtoEnumMap[instance.operation]!,
      'parked_reason': _$ParkReasonEnumMap[instance.parkedReason],
      'post_id': instance.postId,
      'remote_id': instance.remoteId,
      'remote_publish_at': instance.remotePublishAt?.toIso8601String(),
      'remote_url': instance.remoteUrl,
      'state': _$DeliveryStateDtoEnumMap[instance.state]!,
    };

const _$OperationDtoEnumMap = {
  OperationDto.publish: 'publish',
  OperationDto.comment: 'comment',
  OperationDto.direct: 'direct',
  OperationDto.$unknown: r'$unknown',
};

const _$ParkReasonEnumMap = {
  ParkReason.ownerDisabled: 'owner_disabled',
  ParkReason.accountNeedsReconnect: 'account_needs_reconnect',
  ParkReason.platformUnavailable: 'platform_unavailable',
  ParkReason.platformDisabled: 'platform_disabled',
  ParkReason.livePublishDisabled: 'live_publish_disabled',
  ParkReason.quotaExceeded: 'quota_exceeded',
  ParkReason.outcomeUnresolvable: 'outcome_unresolvable',
  ParkReason.$unknown: r'$unknown',
};

const _$DeliveryStateDtoEnumMap = {
  DeliveryStateDto.pending: 'pending',
  DeliveryStateDto.claimed: 'claimed',
  DeliveryStateDto.inFlight: 'in_flight',
  DeliveryStateDto.processing: 'processing',
  DeliveryStateDto.outcomeUnknown: 'outcome_unknown',
  DeliveryStateDto.parked: 'parked',
  DeliveryStateDto.published: 'published',
  DeliveryStateDto.failed: 'failed',
  DeliveryStateDto.deadLetter: 'dead_letter',
  DeliveryStateDto.cancelled: 'cancelled',
  DeliveryStateDto.simulated: 'simulated',
  DeliveryStateDto.$unknown: r'$unknown',
};
