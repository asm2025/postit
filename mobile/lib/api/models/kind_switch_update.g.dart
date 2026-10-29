// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'kind_switch_update.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

KindSwitchUpdate _$KindSwitchUpdateFromJson(Map<String, dynamic> json) =>
    KindSwitchUpdate(
      kind: NotificationKind.fromJson(json['kind'] as String),
      email: json['email'] as bool?,
      push: json['push'] as bool?,
    );

Map<String, dynamic> _$KindSwitchUpdateToJson(KindSwitchUpdate instance) =>
    <String, dynamic>{
      'email': instance.email,
      'kind': _$NotificationKindEnumMap[instance.kind]!,
      'push': instance.push,
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
