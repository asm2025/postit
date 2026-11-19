// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_post_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchPostResponse503 _$PatchPostResponse503FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return PatchPostResponse503(
    code: PatchPostResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: PatchPostResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => PatchPostResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        PatchPostResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : PatchPostResponse503._readExtras(
            PatchPostResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PatchPostResponse503ToJson(
  PatchPostResponse503 instance,
) => <String, dynamic>{
  'code': ?PatchPostResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?PatchPostResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
