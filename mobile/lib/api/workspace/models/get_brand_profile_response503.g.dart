// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_brand_profile_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetBrandProfileResponse503 _$GetBrandProfileResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetBrandProfileResponse503(
    code: GetBrandProfileResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetBrandProfileResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetBrandProfileResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetBrandProfileResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : GetBrandProfileResponse503._readExtras(
            GetBrandProfileResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetBrandProfileResponse503ToJson(
  GetBrandProfileResponse503 instance,
) => <String, dynamic>{
  'code': ?GetBrandProfileResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetBrandProfileResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
