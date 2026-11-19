// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_post_response429.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetPostResponse429 _$GetPostResponse429FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetPostResponse429(
    code: GetPostResponse429._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetPostResponse429._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetPostResponse429ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetPostResponse429._extras(json, 'additionalProperties') == null
        ? const {}
        : GetPostResponse429._readExtras(
            GetPostResponse429._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetPostResponse429ToJson(GetPostResponse429 instance) =>
    <String, dynamic>{
      'code': ?GetPostResponse429._write_code(instance.code),
      'detail': ?instance.detail,
      'field': ?instance.field,
      'missing_scopes': ?instance.missingScopes,
      'request_id': ?instance.requestId,
      'status': ?GetPostResponse429._write_status(instance.status),
      'title': instance.title,
      'type': instance.type,
      'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
    };
