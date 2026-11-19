// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_list_platforms_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminListPlatformsResponse503 _$AdminListPlatformsResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminListPlatformsResponse503(
    code: AdminListPlatformsResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminListPlatformsResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminListPlatformsResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminListPlatformsResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : AdminListPlatformsResponse503._readExtras(
            AdminListPlatformsResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AdminListPlatformsResponse503ToJson(
  AdminListPlatformsResponse503 instance,
) => <String, dynamic>{
  'code': ?AdminListPlatformsResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminListPlatformsResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
