// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validate_post_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidatePostResponse401 _$ValidatePostResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ValidatePostResponse401(
    code: ValidatePostResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ValidatePostResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => ValidatePostResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        ValidatePostResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : ValidatePostResponse401._readExtras(
            ValidatePostResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ValidatePostResponse401ToJson(
  ValidatePostResponse401 instance,
) => <String, dynamic>{
  'code': ?ValidatePostResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?ValidatePostResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
