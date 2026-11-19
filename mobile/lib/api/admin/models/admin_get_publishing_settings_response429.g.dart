// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_get_publishing_settings_response429.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminGetPublishingSettingsResponse429
_$AdminGetPublishingSettingsResponse429FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminGetPublishingSettingsResponse429(
    code: AdminGetPublishingSettingsResponse429._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminGetPublishingSettingsResponse429._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminGetPublishingSettingsResponse429ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminGetPublishingSettingsResponse429._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminGetPublishingSettingsResponse429._readExtras(
            AdminGetPublishingSettingsResponse429._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminGetPublishingSettingsResponse429ToJson(
  AdminGetPublishingSettingsResponse429 instance,
) => <String, dynamic>{
  'code': ?AdminGetPublishingSettingsResponse429._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminGetPublishingSettingsResponse429._write_status(
    instance.status,
  ),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
