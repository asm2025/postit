// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_writing_style_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetWritingStyleResponse500 _$GetWritingStyleResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetWritingStyleResponse500(
    code: GetWritingStyleResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetWritingStyleResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetWritingStyleResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetWritingStyleResponse500._extras(json, 'additionalProperties') == null
        ? const {}
        : GetWritingStyleResponse500._readExtras(
            GetWritingStyleResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetWritingStyleResponse500ToJson(
  GetWritingStyleResponse500 instance,
) => <String, dynamic>{
  'code': ?GetWritingStyleResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetWritingStyleResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
