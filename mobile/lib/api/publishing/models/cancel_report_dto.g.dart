// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelReportDto _$CancelReportDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['canceled', 'not_cancelable', 'requested'],
    disallowNullValues: const ['canceled', 'not_cancelable', 'requested'],
  );
  return CancelReportDto(
    canceled: (json['canceled'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    notCancelable: (json['not_cancelable'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    requested: (json['requested'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    additionalProperties:
        CancelReportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : CancelReportDto._readExtras(
            CancelReportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CancelReportDtoToJson(CancelReportDto instance) =>
    <String, dynamic>{
      'canceled': instance.canceled,
      'not_cancelable': instance.notCancelable,
      'requested': instance.requested,
    };
