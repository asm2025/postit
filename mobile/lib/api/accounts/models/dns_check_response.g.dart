// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'dns_check_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DnsCheckResponse _$DnsCheckResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['findings'],
    disallowNullValues: const ['findings'],
  );
  return DnsCheckResponse(
    findings: (json['findings'] as List<dynamic>)
        .map((e) => DomainFinding.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        DnsCheckResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : DnsCheckResponse._readExtras(
            DnsCheckResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DnsCheckResponseToJson(DnsCheckResponse instance) =>
    <String, dynamic>{
      'findings': instance.findings.map((e) => e.toJson()).toList(),
    };
