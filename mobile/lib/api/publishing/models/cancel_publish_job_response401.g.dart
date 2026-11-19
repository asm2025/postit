// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_publish_job_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelPublishJobResponse401 _$CancelPublishJobResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CancelPublishJobResponse401(
    code: CancelPublishJobResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CancelPublishJobResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CancelPublishJobResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CancelPublishJobResponse401._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : CancelPublishJobResponse401._readExtras(
            CancelPublishJobResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelPublishJobResponse401ToJson(
  CancelPublishJobResponse401 instance,
) => <String, dynamic>{
  'code': ?CancelPublishJobResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CancelPublishJobResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
