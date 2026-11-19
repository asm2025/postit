// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_rotate_vault_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminRotateVaultResponse401 _$AdminRotateVaultResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return AdminRotateVaultResponse401(
    code: AdminRotateVaultResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: AdminRotateVaultResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => AdminRotateVaultResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        AdminRotateVaultResponse401._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : AdminRotateVaultResponse401._readExtras(
            AdminRotateVaultResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AdminRotateVaultResponse401ToJson(
  AdminRotateVaultResponse401 instance,
) => <String, dynamic>{
  'code': ?AdminRotateVaultResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?AdminRotateVaultResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
