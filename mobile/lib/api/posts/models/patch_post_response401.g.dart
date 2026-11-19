// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_post_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchPostResponse401 _$PatchPostResponse401FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return PatchPostResponse401(
    code: PatchPostResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: PatchPostResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => PatchPostResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        PatchPostResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : PatchPostResponse401._readExtras(
            PatchPostResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PatchPostResponse401ToJson(
  PatchPostResponse401 instance,
) => <String, dynamic>{
  'code': ?PatchPostResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?PatchPostResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
