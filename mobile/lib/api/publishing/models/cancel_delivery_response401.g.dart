// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_delivery_response401.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelDeliveryResponse401 _$CancelDeliveryResponse401FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CancelDeliveryResponse401(
    code: CancelDeliveryResponse401._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CancelDeliveryResponse401._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CancelDeliveryResponse401ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CancelDeliveryResponse401._extras(json, 'additionalProperties') == null
        ? const {}
        : CancelDeliveryResponse401._readExtras(
            CancelDeliveryResponse401._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelDeliveryResponse401ToJson(
  CancelDeliveryResponse401 instance,
) => <String, dynamic>{
  'code': ?CancelDeliveryResponse401._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CancelDeliveryResponse401._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
