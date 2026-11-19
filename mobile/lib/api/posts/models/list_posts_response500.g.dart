// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'list_posts_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ListPostsResponse500 _$ListPostsResponse500FromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ListPostsResponse500(
    code: ListPostsResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ListPostsResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => ListPostsResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        ListPostsResponse500._extras(json, 'additionalProperties') == null
        ? const {}
        : ListPostsResponse500._readExtras(
            ListPostsResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ListPostsResponse500ToJson(
  ListPostsResponse500 instance,
) => <String, dynamic>{
  'code': ?ListPostsResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?ListPostsResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
