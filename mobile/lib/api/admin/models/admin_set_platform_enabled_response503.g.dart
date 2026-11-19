// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_set_platform_enabled_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminSetPlatformEnabledResponse503 _$AdminSetPlatformEnabledResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminSetPlatformEnabledResponse503(
    code: AdminSetPlatformEnabledResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminSetPlatformEnabledResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminSetPlatformEnabledResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminSetPlatformEnabledResponse503._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminSetPlatformEnabledResponse503._readExtras(
            AdminSetPlatformEnabledResponse503._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminSetPlatformEnabledResponse503ToJson(
  AdminSetPlatformEnabledResponse503 instance,
) => <String, dynamic>{
  'code': ?AdminSetPlatformEnabledResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminSetPlatformEnabledResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
