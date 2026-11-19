// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delete_post_override_response429.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeletePostOverrideResponse429 _$DeletePostOverrideResponse429FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return DeletePostOverrideResponse429(
    code: DeletePostOverrideResponse429._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: DeletePostOverrideResponse429._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => DeletePostOverrideResponse429ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        DeletePostOverrideResponse429._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : DeletePostOverrideResponse429._readExtras(
            DeletePostOverrideResponse429._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DeletePostOverrideResponse429ToJson(
  DeletePostOverrideResponse429 instance,
) => <String, dynamic>{
  'code': ?DeletePostOverrideResponse429._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?DeletePostOverrideResponse429._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
