// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delivery_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeliveryDto _$DeliveryDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'account_id',
      'attempt_count',
      'id',
      'job_id',
      'operation',
      'post_id',
      'state',
    ],
    disallowNullValues: const [
      'account_id',
      'attempt_count',
      'id',
      'job_id',
      'operation',
      'post_id',
      'state',
    ],
  );
  return DeliveryDto(
    accountId: json['account_id'] as String,
    attemptCount: DeliveryDto._read_attemptCount(json['attempt_count']),
    createdBy: json['created_by'] as String?,
    direct: json['direct'] == null
        ? null
        : DirectCountsDto.fromJson(json['direct'] as Map<String, dynamic>),
    id: json['id'] as String,
    jobId: json['job_id'] as String,
    lastError: json['last_error'] == null
        ? null
        : LastErrorDto.fromJson(json['last_error'] as Map<String, dynamic>),
    nextAttemptAt: DeliveryDto._read_nextAttemptAt(json['next_attempt_at']),
    operation: DeliveryDto._read_operation(json['operation']),
    parkedReason: DeliveryDto._read_parkedReason(json['parked_reason']),
    postId: json['post_id'] as String,
    remoteId: json['remote_id'] as String?,
    remotePublishAt: DeliveryDto._read_remotePublishAt(
      json['remote_publish_at'],
    ),
    remoteUrl: json['remote_url'] as String?,
    state: DeliveryDto._read_state(json['state']),
    additionalProperties:
        DeliveryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : DeliveryDto._readExtras(
            DeliveryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DeliveryDtoToJson(
  DeliveryDto instance,
) => <String, dynamic>{
  'account_id': instance.accountId,
  'attempt_count': ?DeliveryDto._write_attemptCount(instance.attemptCount),
  'created_by': ?instance.createdBy,
  'direct': ?instance.direct?.toJson(),
  'id': instance.id,
  'job_id': instance.jobId,
  'last_error': ?instance.lastError?.toJson(),
  'next_attempt_at': ?DeliveryDto._write_nextAttemptAt(instance.nextAttemptAt),
  'operation': ?DeliveryDto._write_operation(instance.operation),
  'parked_reason': ?DeliveryDto._write_parkedReason(instance.parkedReason),
  'post_id': instance.postId,
  'remote_id': ?instance.remoteId,
  'remote_publish_at': ?DeliveryDto._write_remotePublishAt(
    instance.remotePublishAt,
  ),
  'remote_url': ?instance.remoteUrl,
  'state': ?DeliveryDto._write_state(instance.state),
};
