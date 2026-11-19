// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_set_platform_enabled_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminSetPlatformEnabledResponse500 _$AdminSetPlatformEnabledResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminSetPlatformEnabledResponse500(
    code: AdminSetPlatformEnabledResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminSetPlatformEnabledResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminSetPlatformEnabledResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminSetPlatformEnabledResponse500._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminSetPlatformEnabledResponse500._readExtras(
            AdminSetPlatformEnabledResponse500._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminSetPlatformEnabledResponse500ToJson(
  AdminSetPlatformEnabledResponse500 instance,
) => <String, dynamic>{
  'code': ?AdminSetPlatformEnabledResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminSetPlatformEnabledResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
