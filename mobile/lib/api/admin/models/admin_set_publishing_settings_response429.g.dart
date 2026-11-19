// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_set_publishing_settings_response429.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminSetPublishingSettingsResponse429
_$AdminSetPublishingSettingsResponse429FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminSetPublishingSettingsResponse429(
    code: AdminSetPublishingSettingsResponse429._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminSetPublishingSettingsResponse429._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminSetPublishingSettingsResponse429ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminSetPublishingSettingsResponse429._extras(
              json,
              'additionalProperties',
            ) ==
            null
        ? const {}
        : AdminSetPublishingSettingsResponse429._readExtras(
            AdminSetPublishingSettingsResponse429._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$AdminSetPublishingSettingsResponse429ToJson(
  AdminSetPublishingSettingsResponse429 instance,
) => <String, dynamic>{
  'code': ?AdminSetPublishingSettingsResponse429._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminSetPublishingSettingsResponse429._write_status(
    instance.status,
  ),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
