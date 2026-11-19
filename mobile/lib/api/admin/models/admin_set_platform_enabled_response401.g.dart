// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_set_platform_enabled_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminSetPlatformEnabledResponse401 _$AdminSetPlatformEnabledResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminSetPlatformEnabledResponse401(
    code: AdminSetPlatformEnabledResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminSetPlatformEnabledResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminSetPlatformEnabledResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminSetPlatformEnabledResponse401._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminSetPlatformEnabledResponse401._readExtras(
            AdminSetPlatformEnabledResponse401._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminSetPlatformEnabledResponse401ToJson(
  AdminSetPlatformEnabledResponse401 instance,
) => <String, dynamic>{
  'code': ?AdminSetPlatformEnabledResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminSetPlatformEnabledResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
