// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_get_publishing_settings_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminGetPublishingSettingsResponse401
_$AdminGetPublishingSettingsResponse401FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminGetPublishingSettingsResponse401(
    code: AdminGetPublishingSettingsResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminGetPublishingSettingsResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminGetPublishingSettingsResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminGetPublishingSettingsResponse401._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminGetPublishingSettingsResponse401._readExtras(
            AdminGetPublishingSettingsResponse401._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminGetPublishingSettingsResponse401ToJson(
  AdminGetPublishingSettingsResponse401 instance,
) => <String, dynamic>{
  'code': ?AdminGetPublishingSettingsResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminGetPublishingSettingsResponse401._write_status(
    instance.status,
  ),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
