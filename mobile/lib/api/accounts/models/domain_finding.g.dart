// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'domain_finding.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DomainFinding _$DomainFindingFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'record', 'status'],
    disallowNullValues: const ['code', 'record', 'status'],
  );
  return DomainFinding(
    code: json['code'] as String,
    record: DomainFinding._read_record(json['record']),
    status: DomainFinding._read_status(json['status']),
    additionalProperties:
        DomainFinding._extras(json, 'additionalProperties') == null
        ? const {}
        : DomainFinding._readExtras(
            DomainFinding._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DomainFindingToJson(DomainFinding instance) =>
    <String, dynamic>{
      'code': instance.code,
      'record': ?DomainFinding._write_record(instance.record),
      'status': ?DomainFinding._write_status(instance.status),
    };
