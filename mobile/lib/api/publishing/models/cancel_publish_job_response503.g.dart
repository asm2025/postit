// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_publish_job_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelPublishJobResponse503 _$CancelPublishJobResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CancelPublishJobResponse503(
    code: CancelPublishJobResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CancelPublishJobResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CancelPublishJobResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CancelPublishJobResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : CancelPublishJobResponse503._readExtras(
            CancelPublishJobResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelPublishJobResponse503ToJson(
  CancelPublishJobResponse503 instance,
) => <String, dynamic>{
  'code': ?CancelPublishJobResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CancelPublishJobResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
