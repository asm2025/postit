// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_brand_profile_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetBrandProfileResponse401 _$GetBrandProfileResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetBrandProfileResponse401(
    code: GetBrandProfileResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetBrandProfileResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetBrandProfileResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetBrandProfileResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : GetBrandProfileResponse401._readExtras(
            GetBrandProfileResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetBrandProfileResponse401ToJson(
  GetBrandProfileResponse401 instance,
) => <String, dynamic>{
  'code': ?GetBrandProfileResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetBrandProfileResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
