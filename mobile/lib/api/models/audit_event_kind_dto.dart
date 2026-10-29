// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Every audit event kind (`AuditEventKind` in `postit-data`); the `OpenAPI` enum the web.
/// client's kind filter is typed from. Clients must tolerate values added later.
@JsonEnum()
enum AuditEventKindDto {
  @JsonValue('user_provisioned')
  userProvisioned('user_provisioned'),
  @JsonValue('user_approved')
  userApproved('user_approved'),
  @JsonValue('user_disabled')
  userDisabled('user_disabled'),
  @JsonValue('user_enabled')
  userEnabled('user_enabled'),
  @JsonValue('role_changed')
  roleChanged('role_changed'),
  @JsonValue('bootstrap_admin_granted')
  bootstrapAdminGranted('bootstrap_admin_granted'),
  @JsonValue('user_deleted')
  userDeleted('user_deleted'),
  @JsonValue('email_sent')
  emailSent('email_sent'),
  @JsonValue('email_dropped')
  emailDropped('email_dropped'),
  @JsonValue('email_failed')
  emailFailed('email_failed'),
  @JsonValue('job_retried')
  jobRetried('job_retried'),
  @JsonValue('job_cancelled')
  jobCancelled('job_cancelled'),
  @JsonValue('job_deleted')
  jobDeleted('job_deleted'),
  @JsonValue('job_triggered')
  jobTriggered('job_triggered'),
  @JsonValue('vault_rotation_started')
  vaultRotationStarted('vault_rotation_started'),
  @JsonValue('vault_rotation_completed')
  vaultRotationCompleted('vault_rotation_completed'),
  @JsonValue('account_connected')
  accountConnected('account_connected'),
  @JsonValue('account_disconnected')
  accountDisconnected('account_disconnected'),
  @JsonValue('account_needs_reconnect')
  accountNeedsReconnect('account_needs_reconnect'),
  @JsonValue('accounts_imported')
  accountsImported('accounts_imported'),
  @JsonValue('account_revoked_by_platform')
  accountRevokedByPlatform('account_revoked_by_platform'),
  @JsonValue('delegation_granted')
  delegationGranted('delegation_granted'),
  @JsonValue('delegation_changed')
  delegationChanged('delegation_changed'),
  @JsonValue('delegation_revoked')
  delegationRevoked('delegation_revoked'),
  @JsonValue('delegation_left')
  delegationLeft('delegation_left'),
  @JsonValue('delegation_expired')
  delegationExpired('delegation_expired'),
  @JsonValue('publish_confirmed')
  publishConfirmed('publish_confirmed'),
  @JsonValue('publish_job_rescheduled')
  publishJobRescheduled('publish_job_rescheduled'),
  @JsonValue('publish_job_cancelled')
  publishJobCancelled('publish_job_cancelled'),
  @JsonValue('delivery_cancelled')
  deliveryCancelled('delivery_cancelled'),
  @JsonValue('delivery_retried')
  deliveryRetried('delivery_retried'),
  @JsonValue('delivery_resolved')
  deliveryResolved('delivery_resolved'),
  @JsonValue('delivery_published')
  deliveryPublished('delivery_published'),
  @JsonValue('delivery_dead_lettered')
  deliveryDeadLettered('delivery_dead_lettered'),
  @JsonValue('delivery_outcome_unknown')
  deliveryOutcomeUnknown('delivery_outcome_unknown'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const AuditEventKindDto(this.json);

  factory AuditEventKindDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<AuditEventKindDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
