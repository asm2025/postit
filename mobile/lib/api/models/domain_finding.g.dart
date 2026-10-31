// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'domain_finding.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DomainFinding _$DomainFindingFromJson(Map<String, dynamic> json) =>
    DomainFinding(
      code: json['code'] as String,
      record: DomainRecord.fromJson(json['record'] as String),
      status: DomainStatus.fromJson(json['status'] as String),
    );

Map<String, dynamic> _$DomainFindingToJson(DomainFinding instance) =>
    <String, dynamic>{
      'code': instance.code,
      'record': _$DomainRecordEnumMap[instance.record]!,
      'status': _$DomainStatusEnumMap[instance.status]!,
    };

const _$DomainRecordEnumMap = {
  DomainRecord.spf: 'spf',
  DomainRecord.dkim: 'dkim',
  DomainRecord.dmarc: 'dmarc',
  DomainRecord.$unknown: r'$unknown',
};

const _$DomainStatusEnumMap = {
  DomainStatus.ok: 'ok',
  DomainStatus.missing: 'missing',
  DomainStatus.weak: 'weak',
  DomainStatus.unknown: 'unknown',
  DomainStatus.$unknown: r'$unknown',
};
