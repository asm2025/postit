// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_platform_icon_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetPlatformIconResponse503 _$GetPlatformIconResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetPlatformIconResponse503(
    code: GetPlatformIconResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetPlatformIconResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetPlatformIconResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetPlatformIconResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : GetPlatformIconResponse503._readExtras(
            GetPlatformIconResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetPlatformIconResponse503ToJson(
  GetPlatformIconResponse503 instance,
) => <String, dynamic>{
  'code': ?GetPlatformIconResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetPlatformIconResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
