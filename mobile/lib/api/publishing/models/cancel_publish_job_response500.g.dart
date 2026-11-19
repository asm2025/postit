// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_publish_job_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelPublishJobResponse500 _$CancelPublishJobResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CancelPublishJobResponse500(
    code: CancelPublishJobResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CancelPublishJobResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CancelPublishJobResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CancelPublishJobResponse500._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : CancelPublishJobResponse500._readExtras(
            CancelPublishJobResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelPublishJobResponse500ToJson(
  CancelPublishJobResponse500 instance,
) => <String, dynamic>{
  'code': ?CancelPublishJobResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CancelPublishJobResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
