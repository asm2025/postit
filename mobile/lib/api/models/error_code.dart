// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Machine-readable problem code (the `code` member of every problem+json body).
@JsonEnum()
enum ErrorCode {
  @JsonValue('unauthenticated')
  unauthenticated('unauthenticated'),
  @JsonValue('account_pending')
  accountPending('account_pending'),
  @JsonValue('account_disabled')
  accountDisabled('account_disabled'),
  @JsonValue('forbidden')
  forbidden('forbidden'),
  @JsonValue('owner_only')
  ownerOnly('owner_only'),
  @JsonValue('capability_required')
  capabilityRequired('capability_required'),
  @JsonValue('delegation_invalid')
  delegationInvalid('delegation_invalid'),
  @JsonValue('account_not_in_scope')
  accountNotInScope('account_not_in_scope'),
  @JsonValue('not_found')
  notFound('not_found'),
  @JsonValue('push_disabled')
  pushDisabled('push_disabled'),
  @JsonValue('request_timeout')
  requestTimeout('request_timeout'),
  @JsonValue('user_deleting')
  userDeleting('user_deleting'),
  @JsonValue('last_admin')
  lastAdmin('last_admin'),
  @JsonValue('idempotency_in_progress')
  idempotencyInProgress('idempotency_in_progress'),
  @JsonValue('job_action_not_allowed')
  jobActionNotAllowed('job_action_not_allowed'),
  @JsonValue('platform_disabled')
  platformDisabled('platform_disabled'),
  @JsonValue('capability_unsupported')
  capabilityUnsupported('capability_unsupported'),
  @JsonValue('sending_method_exists')
  sendingMethodExists('sending_method_exists'),
  @JsonValue('delegation_exists')
  delegationExists('delegation_exists'),
  @JsonValue('job_active')
  jobActive('job_active'),
  @JsonValue('media_in_use')
  mediaInUse('media_in_use'),
  @JsonValue('upload_offset_mismatch')
  uploadOffsetMismatch('upload_offset_mismatch'),
  @JsonValue('version_conflict')
  versionConflict('version_conflict'),
  @JsonValue('payload_too_large')
  payloadTooLarge('payload_too_large'),
  @JsonValue('storage_quota_exceeded')
  storageQuotaExceeded('storage_quota_exceeded'),
  @JsonValue('unsupported_media_type')
  unsupportedMediaType('unsupported_media_type'),
  @JsonValue('validation_failed')
  validationFailed('validation_failed'),
  @JsonValue('credentials_invalid')
  credentialsInvalid('credentials_invalid'),
  @JsonValue('idempotency_key_reused')
  idempotencyKeyReused('idempotency_key_reused'),
  @JsonValue('return_to_invalid')
  returnToInvalid('return_to_invalid'),
  @JsonValue('push_endpoint_invalid')
  pushEndpointInvalid('push_endpoint_invalid'),
  @JsonValue('precondition_required')
  preconditionRequired('precondition_required'),
  @JsonValue('rate_limited')
  rateLimited('rate_limited'),
  @JsonValue('internal')
  internal('internal'),
  @JsonValue('unavailable')
  unavailable('unavailable'),
  @JsonValue('platform_unavailable')
  platformUnavailable('platform_unavailable'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const ErrorCode(this.json);

  factory ErrorCode.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<ErrorCode> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
