// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_delivery_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelDeliveryResponse503 _$CancelDeliveryResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return CancelDeliveryResponse503(
    code: CancelDeliveryResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: CancelDeliveryResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => CancelDeliveryResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        CancelDeliveryResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : CancelDeliveryResponse503._readExtras(
            CancelDeliveryResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelDeliveryResponse503ToJson(
  CancelDeliveryResponse503 instance,
) => <String, dynamic>{
  'code': ?CancelDeliveryResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?CancelDeliveryResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
