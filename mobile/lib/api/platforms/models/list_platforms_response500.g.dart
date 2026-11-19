// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'list_platforms_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ListPlatformsResponse500 _$ListPlatformsResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ListPlatformsResponse500(
    code: ListPlatformsResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ListPlatformsResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => ListPlatformsResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        ListPlatformsResponse500._extras(json, 'additionalProperties') == null
        ? const {}
        : ListPlatformsResponse500._readExtras(
            ListPlatformsResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ListPlatformsResponse500ToJson(
  ListPlatformsResponse500 instance,
) => <String, dynamic>{
  'code': ?ListPlatformsResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?ListPlatformsResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
