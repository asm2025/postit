// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'problem_details.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ProblemDetails _$ProblemDetailsFromJson(Map<String, dynamic> json) =>
    ProblemDetails(
      code: ErrorCode.fromJson(json['code'] as String),
      status: (json['status'] as num).toInt(),
      title: json['title'] as String,
      type: json['type'] as String,
      detail: json['detail'] as String?,
      field: json['field'] as String?,
      missingScopes: (json['missing_scopes'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
      requestId: json['request_id'] as String?,
      violations: json['violations'] as List<dynamic>?,
    );

Map<String, dynamic> _$ProblemDetailsToJson(ProblemDetails instance) =>
    <String, dynamic>{
      'code': _$ErrorCodeEnumMap[instance.code]!,
      'detail': instance.detail,
      'field': instance.field,
      'missing_scopes': instance.missingScopes,
      'request_id': instance.requestId,
      'status': instance.status,
      'title': instance.title,
      'type': instance.type,
      'violations': instance.violations,
    };

const _$ErrorCodeEnumMap = {
  ErrorCode.consentAttestationRequired: 'consent_attestation_required',
  ErrorCode.confirmationRequired: 'confirmation_required',
  ErrorCode.acknowledgmentRequired: 'acknowledgment_required',
  ErrorCode.unauthenticated: 'unauthenticated',
  ErrorCode.accountPending: 'account_pending',
  ErrorCode.accountDisabled: 'account_disabled',
  ErrorCode.forbidden: 'forbidden',
  ErrorCode.ownerOnly: 'owner_only',
  ErrorCode.capabilityRequired: 'capability_required',
  ErrorCode.delegationInvalid: 'delegation_invalid',
  ErrorCode.accountNotInScope: 'account_not_in_scope',
  ErrorCode.notFound: 'not_found',
  ErrorCode.pushDisabled: 'push_disabled',
  ErrorCode.requestTimeout: 'request_timeout',
  ErrorCode.userDeleting: 'user_deleting',
  ErrorCode.lastAdmin: 'last_admin',
  ErrorCode.idempotencyInProgress: 'idempotency_in_progress',
  ErrorCode.jobActionNotAllowed: 'job_action_not_allowed',
  ErrorCode.platformDisabled: 'platform_disabled',
  ErrorCode.capabilityUnsupported: 'capability_unsupported',
  ErrorCode.sendingMethodExists: 'sending_method_exists',
  ErrorCode.delegationExists: 'delegation_exists',
  ErrorCode.jobActive: 'job_active',
  ErrorCode.mediaInUse: 'media_in_use',
  ErrorCode.uploadOffsetMismatch: 'upload_offset_mismatch',
  ErrorCode.contactLimitExceeded: 'contact_limit_exceeded',
  ErrorCode.suppressed: 'suppressed',
  ErrorCode.suppressionNotRemovable: 'suppression_not_removable',
  ErrorCode.statusNotSettable: 'status_not_settable',
  ErrorCode.emailAddressMissing: 'email_address_missing',
  ErrorCode.livePublishDisabled: 'live_publish_disabled',
  ErrorCode.testSendRefused: 'test_send_refused',
  ErrorCode.aiProviderRequired: 'ai_provider_required',
  ErrorCode.notReschedulable: 'not_reschedulable',
  ErrorCode.notRetryable: 'not_retryable',
  ErrorCode.notResolvable: 'not_resolvable',
  ErrorCode.postLocked: 'post_locked',
  ErrorCode.rotationInProgress: 'rotation_in_progress',
  ErrorCode.versionConflict: 'version_conflict',
  ErrorCode.payloadTooLarge: 'payload_too_large',
  ErrorCode.storageQuotaExceeded: 'storage_quota_exceeded',
  ErrorCode.csvTooLarge: 'csv_too_large',
  ErrorCode.unsupportedMediaType: 'unsupported_media_type',
  ErrorCode.validationFailed: 'validation_failed',
  ErrorCode.credentialsInvalid: 'credentials_invalid',
  ErrorCode.idempotencyKeyReused: 'idempotency_key_reused',
  ErrorCode.returnToInvalid: 'return_to_invalid',
  ErrorCode.pushEndpointInvalid: 'push_endpoint_invalid',
  ErrorCode.csvInvalid: 'csv_invalid',
  ErrorCode.preconditionRequired: 'precondition_required',
  ErrorCode.rateLimited: 'rate_limited',
  ErrorCode.internal: 'internal',
  ErrorCode.aiProviderFailed: 'ai_provider_failed',
  ErrorCode.unavailable: 'unavailable',
  ErrorCode.platformUnavailable: 'platform_unavailable',
  ErrorCode.$unknown: r'$unknown',
};
