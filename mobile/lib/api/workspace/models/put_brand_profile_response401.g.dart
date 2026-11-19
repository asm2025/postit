// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_brand_profile_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutBrandProfileResponse401 _$PutBrandProfileResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return PutBrandProfileResponse401(
    code: PutBrandProfileResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: PutBrandProfileResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => PutBrandProfileResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        PutBrandProfileResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : PutBrandProfileResponse401._readExtras(
            PutBrandProfileResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PutBrandProfileResponse401ToJson(
  PutBrandProfileResponse401 instance,
) => <String, dynamic>{
  'code': ?PutBrandProfileResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?PutBrandProfileResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
