// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_list_platforms_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminListPlatformsResponse401 _$AdminListPlatformsResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminListPlatformsResponse401(
    code: AdminListPlatformsResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminListPlatformsResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminListPlatformsResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminListPlatformsResponse401._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : AdminListPlatformsResponse401._readExtras(
            AdminListPlatformsResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AdminListPlatformsResponse401ToJson(
  AdminListPlatformsResponse401 instance,
) => <String, dynamic>{
  'code': ?AdminListPlatformsResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminListPlatformsResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
