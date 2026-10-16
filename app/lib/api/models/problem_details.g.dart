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
      requestId: json['request_id'] as String?,
    );

Map<String, dynamic> _$ProblemDetailsToJson(ProblemDetails instance) =>
    <String, dynamic>{
      'code': _$ErrorCodeEnumMap[instance.code]!,
      'detail': instance.detail,
      'request_id': instance.requestId,
      'status': instance.status,
      'title': instance.title,
      'type': instance.type,
    };

const _$ErrorCodeEnumMap = {
  ErrorCode.unauthenticated: 'unauthenticated',
  ErrorCode.accountPending: 'account_pending',
  ErrorCode.accountDisabled: 'account_disabled',
  ErrorCode.forbidden: 'forbidden',
  ErrorCode.notFound: 'not_found',
  ErrorCode.requestTimeout: 'request_timeout',
  ErrorCode.userDeleting: 'user_deleting',
  ErrorCode.lastAdmin: 'last_admin',
  ErrorCode.idempotencyInProgress: 'idempotency_in_progress',
  ErrorCode.jobActionNotAllowed: 'job_action_not_allowed',
  ErrorCode.versionConflict: 'version_conflict',
  ErrorCode.payloadTooLarge: 'payload_too_large',
  ErrorCode.validationFailed: 'validation_failed',
  ErrorCode.idempotencyKeyReused: 'idempotency_key_reused',
  ErrorCode.preconditionRequired: 'precondition_required',
  ErrorCode.rateLimited: 'rate_limited',
  ErrorCode.internal: 'internal',
  ErrorCode.unavailable: 'unavailable',
  ErrorCode.$unknown: r'$unknown',
};
