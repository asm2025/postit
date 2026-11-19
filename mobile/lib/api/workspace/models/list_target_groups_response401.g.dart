// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'list_target_groups_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ListTargetGroupsResponse401 _$ListTargetGroupsResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ListTargetGroupsResponse401(
    code: ListTargetGroupsResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ListTargetGroupsResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => ListTargetGroupsResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        ListTargetGroupsResponse401._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : ListTargetGroupsResponse401._readExtras(
            ListTargetGroupsResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ListTargetGroupsResponse401ToJson(
  ListTargetGroupsResponse401 instance,
) => <String, dynamic>{
  'code': ?ListTargetGroupsResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?ListTargetGroupsResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
