// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_target_group_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateTargetGroupResponse503 _$CreateTargetGroupResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CreateTargetGroupResponse503(
    code: CreateTargetGroupResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CreateTargetGroupResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CreateTargetGroupResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CreateTargetGroupResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : CreateTargetGroupResponse503._readExtras(
            CreateTargetGroupResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CreateTargetGroupResponse503ToJson(
  CreateTargetGroupResponse503 instance,
) => <String, dynamic>{
  'code': ?CreateTargetGroupResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CreateTargetGroupResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
