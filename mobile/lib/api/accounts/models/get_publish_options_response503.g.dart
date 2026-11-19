// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_publish_options_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetPublishOptionsResponse503 _$GetPublishOptionsResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetPublishOptionsResponse503(
    code: GetPublishOptionsResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetPublishOptionsResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetPublishOptionsResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetPublishOptionsResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : GetPublishOptionsResponse503._readExtras(
            GetPublishOptionsResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetPublishOptionsResponse503ToJson(
  GetPublishOptionsResponse503 instance,
) => <String, dynamic>{
  'code': ?GetPublishOptionsResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetPublishOptionsResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
