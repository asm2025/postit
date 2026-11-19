// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delete_target_group_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeleteTargetGroupResponse500 _$DeleteTargetGroupResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return DeleteTargetGroupResponse500(
    code: DeleteTargetGroupResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: DeleteTargetGroupResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => DeleteTargetGroupResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        DeleteTargetGroupResponse500._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : DeleteTargetGroupResponse500._readExtras(
            DeleteTargetGroupResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DeleteTargetGroupResponse500ToJson(
  DeleteTargetGroupResponse500 instance,
) => <String, dynamic>{
  'code': ?DeleteTargetGroupResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?DeleteTargetGroupResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
