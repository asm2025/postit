// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'data6.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Data6 _$Data6FromJson(Map<String, dynamic> json) => Data6(
  createdAt: DateTime.parse(json['created_at'] as String),
  id: json['id'] as String,
  kind: NotificationKind.fromJson(json['kind'] as String),
  payload: json['payload'],
  severity: Severity.fromJson(json['severity'] as String),
  ownerId: json['owner_id'] as String?,
  readAt: json['read_at'] == null
      ? null
      : DateTime.parse(json['read_at'] as String),
);

Map<String, dynamic> _$Data6ToJson(Data6 instance) => <String, dynamic>{
  'created_at': instance.createdAt.toIso8601String(),
  'id': instance.id,
  'kind': _$NotificationKindEnumMap[instance.kind]!,
  'owner_id': instance.ownerId,
  'payload': instance.payload,
  'read_at': instance.readAt?.toIso8601String(),
  'severity': _$SeverityEnumMap[instance.severity]!,
};

const _$NotificationKindEnumMap = {
  NotificationKind.deliveryDeadLettered: 'delivery_dead_lettered',
  NotificationKind.deliveryFailed: 'delivery_failed',
  NotificationKind.deliveryOutcomeUnknown: 'delivery_outcome_unknown',
  NotificationKind.deliveryParked: 'delivery_parked',
  NotificationKind.deliveryStale: 'delivery_stale',
  NotificationKind.jobPartial: 'job_partial',
  NotificationKind.accountNeedsReconnect: 'account_needs_reconnect',
  NotificationKind.accountRevokedByPlatform: 'account_revoked_by_platform',
  NotificationKind.reconnectByPassed: 'reconnect_by_passed',
  NotificationKind.recurrenceSkipped: 'recurrence_skipped',
  NotificationKind.deliveryPublished: 'delivery_published',
  NotificationKind.reconnectReminder: 'reconnect_reminder',
  NotificationKind.delegationChanged: 'delegation_changed',
  NotificationKind.newComments: 'new_comments',
  NotificationKind.userPending: 'user_pending',
  NotificationKind.$unknown: r'$unknown',
};

const _$SeverityEnumMap = {
  Severity.info: 'info',
  Severity.critical: 'critical',
  Severity.$unknown: r'$unknown',
};
