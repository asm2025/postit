// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_post_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreatePostResponse500 _$CreatePostResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CreatePostResponse500(
    code: CreatePostResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CreatePostResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CreatePostResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CreatePostResponse500._extras(json, 'additionalProperties') == null
        ? const {}
        : CreatePostResponse500._readExtras(
            CreatePostResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CreatePostResponse500ToJson(
  CreatePostResponse500 instance,
) => <String, dynamic>{
  'code': ?CreatePostResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CreatePostResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
