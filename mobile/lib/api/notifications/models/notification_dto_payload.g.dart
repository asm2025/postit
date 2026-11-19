// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'notification_dto_payload.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AccountNotice _$AccountNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account'],
    disallowNullValues: const ['account'],
  );
  return AccountNotice(
    account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
    additionalProperties:
        AccountNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : AccountNotice._readExtras(
            AccountNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AccountNoticeToJson(AccountNotice instance) =>
    <String, dynamic>{'account': instance.account.toJson()};

CommentsNotice _$CommentsNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account', 'count'],
    disallowNullValues: const ['account', 'count'],
  );
  return CommentsNotice(
    account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
    count: CommentsNotice._read_count(json['count']),
    additionalProperties:
        CommentsNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : CommentsNotice._readExtras(
            CommentsNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CommentsNoticeToJson(CommentsNotice instance) =>
    <String, dynamic>{
      'account': instance.account.toJson(),
      'count': ?CommentsNotice._write_count(instance.count),
    };

DelegationNotice _$DelegationNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['delegation_id', 'event', 'party_name'],
    disallowNullValues: const ['event', 'party_name'],
  );
  return DelegationNotice(
    delegationId: DelegationNotice._read_delegationId(json['delegation_id']),
    event: DelegationNotice._read_event(json['event']),
    partyName: json['party_name'] as String,
    additionalProperties:
        DelegationNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : DelegationNotice._readExtras(
            DelegationNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DelegationNoticeToJson(
  DelegationNotice instance,
) => <String, dynamic>{
  'delegation_id': DelegationNotice._write_delegationId(instance.delegationId),
  'event': ?DelegationNotice._write_event(instance.event),
  'party_name': instance.partyName,
};

DeliveryNotice _$DeliveryNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account', 'delivery_id', 'job_id', 'post_id'],
    disallowNullValues: const ['account'],
  );
  return DeliveryNotice(
    account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
    deliveryId: DeliveryNotice._read_deliveryId(json['delivery_id']),
    jobId: DeliveryNotice._read_jobId(json['job_id']),
    parkReason: DeliveryNotice._read_parkReason(json['park_reason']),
    postId: DeliveryNotice._read_postId(json['post_id']),
    additionalProperties:
        DeliveryNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : DeliveryNotice._readExtras(
            DeliveryNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DeliveryNoticeToJson(DeliveryNotice instance) =>
    <String, dynamic>{
      'account': instance.account.toJson(),
      'delivery_id': DeliveryNotice._write_deliveryId(instance.deliveryId),
      'job_id': DeliveryNotice._write_jobId(instance.jobId),
      'park_reason': ?DeliveryNotice._write_parkReason(instance.parkReason),
      'post_id': DeliveryNotice._write_postId(instance.postId),
    };

JobNotice _$JobNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['failed', 'job_id', 'post_id', 'published'],
    disallowNullValues: const ['failed', 'published'],
  );
  return JobNotice(
    failed: JobNotice._read_failed(json['failed']),
    jobId: JobNotice._read_jobId(json['job_id']),
    postId: JobNotice._read_postId(json['post_id']),
    published: JobNotice._read_published(json['published']),
    additionalProperties:
        JobNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : JobNotice._readExtras(
            JobNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobNoticeToJson(JobNotice instance) => <String, dynamic>{
  'failed': ?JobNotice._write_failed(instance.failed),
  'job_id': JobNotice._write_jobId(instance.jobId),
  'post_id': JobNotice._write_postId(instance.postId),
  'published': ?JobNotice._write_published(instance.published),
};

ReconnectNotice _$ReconnectNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account', 'grant_id', 'reconnect_by'],
    disallowNullValues: const ['account', 'reconnect_by'],
  );
  return ReconnectNotice(
    account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
    grantId: ReconnectNotice._read_grantId(json['grant_id']),
    reconnectBy: ReconnectNotice._read_reconnectBy(json['reconnect_by']),
    stage: ReconnectNotice._read_stage(json['stage']),
    additionalProperties:
        ReconnectNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : ReconnectNotice._readExtras(
            ReconnectNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ReconnectNoticeToJson(ReconnectNotice instance) =>
    <String, dynamic>{
      'account': instance.account.toJson(),
      'grant_id': ReconnectNotice._write_grantId(instance.grantId),
      'reconnect_by': ?ReconnectNotice._write_reconnectBy(instance.reconnectBy),
      'stage': ?ReconnectNotice._write_stage(instance.stage),
    };

RecurrenceNotice _$RecurrenceNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['post_id', 'reason', 'recurrence_id'],
    disallowNullValues: const ['reason'],
  );
  return RecurrenceNotice(
    postId: RecurrenceNotice._read_postId(json['post_id']),
    reason: RecurrenceNotice._read_reason(json['reason']),
    recurrenceId: RecurrenceNotice._read_recurrenceId(json['recurrence_id']),
    additionalProperties:
        RecurrenceNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : RecurrenceNotice._readExtras(
            RecurrenceNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$RecurrenceNoticeToJson(
  RecurrenceNotice instance,
) => <String, dynamic>{
  'post_id': RecurrenceNotice._write_postId(instance.postId),
  'reason': ?RecurrenceNotice._write_reason(instance.reason),
  'recurrence_id': RecurrenceNotice._write_recurrenceId(instance.recurrenceId),
};

UserPendingNotice _$UserPendingNoticeFromJson(Map<String, dynamic> json) {
  $checkKeys(json, requiredKeys: const ['user_id']);
  return UserPendingNotice(
    userId: UserPendingNotice._read_userId(json['user_id']),
    additionalProperties:
        UserPendingNotice._extras(json, 'additionalProperties') == null
        ? const {}
        : UserPendingNotice._readExtras(
            UserPendingNotice._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$UserPendingNoticeToJson(UserPendingNotice instance) =>
    <String, dynamic>{
      'user_id': UserPendingNotice._write_userId(instance.userId),
    };
