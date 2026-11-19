// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetReportDto _$TargetReportDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_id', 'violations'],
    disallowNullValues: const ['account_id', 'violations'],
  );
  return TargetReportDto(
    accountId: json['account_id'] as String,
    format: TargetReportDto._read_format(json['format']),
    violations: (json['violations'] as List<dynamic>)
        .map((e) => Violation.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        TargetReportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : TargetReportDto._readExtras(
            TargetReportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$TargetReportDtoToJson(TargetReportDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'format': ?TargetReportDto._write_format(instance.format),
      'violations': instance.violations.map((e) => e.toJson()).toList(),
    };
