// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_post_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreatePostResponse401 _$CreatePostResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CreatePostResponse401(
    code: CreatePostResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CreatePostResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CreatePostResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CreatePostResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : CreatePostResponse401._readExtras(
            CreatePostResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CreatePostResponse401ToJson(
  CreatePostResponse401 instance,
) => <String, dynamic>{
  'code': ?CreatePostResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CreatePostResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
