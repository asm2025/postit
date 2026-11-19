// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_get_publishing_settings_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminGetPublishingSettingsResponse503
_$AdminGetPublishingSettingsResponse503FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminGetPublishingSettingsResponse503(
    code: AdminGetPublishingSettingsResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminGetPublishingSettingsResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminGetPublishingSettingsResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminGetPublishingSettingsResponse503._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminGetPublishingSettingsResponse503._readExtras(
            AdminGetPublishingSettingsResponse503._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminGetPublishingSettingsResponse503ToJson(
  AdminGetPublishingSettingsResponse503 instance,
) => <String, dynamic>{
  'code': ?AdminGetPublishingSettingsResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminGetPublishingSettingsResponse503._write_status(
    instance.status,
  ),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
