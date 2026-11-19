// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_put_user_limits_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminPutUserLimitsResponse503 _$AdminPutUserLimitsResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminPutUserLimitsResponse503(
    code: AdminPutUserLimitsResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminPutUserLimitsResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminPutUserLimitsResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminPutUserLimitsResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : AdminPutUserLimitsResponse503._readExtras(
            AdminPutUserLimitsResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AdminPutUserLimitsResponse503ToJson(
  AdminPutUserLimitsResponse503 instance,
) => <String, dynamic>{
  'code': ?AdminPutUserLimitsResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminPutUserLimitsResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
