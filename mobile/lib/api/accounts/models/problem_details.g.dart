// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'problem_details.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ProblemDetails _$ProblemDetailsFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ProblemDetails(
    code: ProblemDetails._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ProblemDetails._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) =>
              ProblemDetailsViolationsItem.fromJson(e as Map<String, dynamic>),
        )
        .toList(),
    additionalProperties:
        ProblemDetails._extras(json, 'additionalProperties') == null
        ? const {}
        : ProblemDetails._readExtras(
            ProblemDetails._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ProblemDetailsToJson(ProblemDetails instance) =>
    <String, dynamic>{
      'code': ?ProblemDetails._write_code(instance.code),
      'detail': ?instance.detail,
      'field': ?instance.field,
      'missing_scopes': ?instance.missingScopes,
      'request_id': ?instance.requestId,
      'status': ?ProblemDetails._write_status(instance.status),
      'title': instance.title,
      'type': instance.type,
      'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
    };
