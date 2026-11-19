// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_writing_style_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutWritingStyleResponse401 _$PutWritingStyleResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return PutWritingStyleResponse401(
    code: PutWritingStyleResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: PutWritingStyleResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => PutWritingStyleResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        PutWritingStyleResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : PutWritingStyleResponse401._readExtras(
            PutWritingStyleResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PutWritingStyleResponse401ToJson(
  PutWritingStyleResponse401 instance,
) => <String, dynamic>{
  'code': ?PutWritingStyleResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?PutWritingStyleResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
