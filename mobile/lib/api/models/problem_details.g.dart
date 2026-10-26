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
  ErrorCode.unauthenticated: 'unauthenticated',
  ErrorCode.accountPending: 'account_pending',
  ErrorCode.accountDisabled: 'account_disabled',
  ErrorCode.forbidden: 'forbidden',
  ErrorCode.ownerOnly: 'owner_only',
  ErrorCode.capabilityRequired: 'capability_required',
  ErrorCode.delegationInvalid: 'delegation_invalid',
  ErrorCode.notFound: 'not_found',
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
  ErrorCode.versionConflict: 'version_conflict',
  ErrorCode.payloadTooLarge: 'payload_too_large',
  ErrorCode.storageQuotaExceeded: 'storage_quota_exceeded',
  ErrorCode.unsupportedMediaType: 'unsupported_media_type',
  ErrorCode.validationFailed: 'validation_failed',
  ErrorCode.credentialsInvalid: 'credentials_invalid',
  ErrorCode.idempotencyKeyReused: 'idempotency_key_reused',
  ErrorCode.preconditionRequired: 'precondition_required',
  ErrorCode.rateLimited: 'rate_limited',
  ErrorCode.internal: 'internal',
  ErrorCode.unavailable: 'unavailable',
  ErrorCode.platformUnavailable: 'platform_unavailable',
  ErrorCode.$unknown: r'$unknown',
};
