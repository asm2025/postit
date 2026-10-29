// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Every kind of notification.
@JsonEnum()
enum NotificationKind {
  @JsonValue('delivery_dead_lettered')
  deliveryDeadLettered('delivery_dead_lettered'),
  @JsonValue('delivery_failed')
  deliveryFailed('delivery_failed'),
  @JsonValue('delivery_outcome_unknown')
  deliveryOutcomeUnknown('delivery_outcome_unknown'),
  @JsonValue('delivery_parked')
  deliveryParked('delivery_parked'),
  @JsonValue('delivery_stale')
  deliveryStale('delivery_stale'),
  @JsonValue('job_partial')
  jobPartial('job_partial'),
  @JsonValue('account_needs_reconnect')
  accountNeedsReconnect('account_needs_reconnect'),
  @JsonValue('account_revoked_by_platform')
  accountRevokedByPlatform('account_revoked_by_platform'),
  @JsonValue('reconnect_by_passed')
  reconnectByPassed('reconnect_by_passed'),
  @JsonValue('recurrence_skipped')
  recurrenceSkipped('recurrence_skipped'),
  @JsonValue('delivery_published')
  deliveryPublished('delivery_published'),
  @JsonValue('reconnect_reminder')
  reconnectReminder('reconnect_reminder'),
  @JsonValue('delegation_changed')
  delegationChanged('delegation_changed'),
  @JsonValue('new_comments')
  newComments('new_comments'),
  @JsonValue('user_pending')
  userPending('user_pending'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const NotificationKind(this.json);

  factory NotificationKind.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<NotificationKind> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
