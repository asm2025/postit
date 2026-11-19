// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_post_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetPostResponse503 _$GetPostResponse503FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetPostResponse503(
    code: GetPostResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetPostResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetPostResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetPostResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : GetPostResponse503._readExtras(
            GetPostResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetPostResponse503ToJson(GetPostResponse503 instance) =>
    <String, dynamic>{
      'code': ?GetPostResponse503._write_code(instance.code),
      'detail': ?instance.detail,
      'field': ?instance.field,
      'missing_scopes': ?instance.missingScopes,
      'request_id': ?instance.requestId,
      'status': ?GetPostResponse503._write_status(instance.status),
      'title': instance.title,
      'type': instance.type,
      'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
    };
