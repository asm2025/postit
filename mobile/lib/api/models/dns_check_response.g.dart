// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'dns_check_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DnsCheckResponse _$DnsCheckResponseFromJson(Map<String, dynamic> json) =>
    DnsCheckResponse(
      findings: (json['findings'] as List<dynamic>)
          .map((e) => DomainFinding.fromJson(e as Map<String, dynamic>))
          .toList(),
    );

Map<String, dynamic> _$DnsCheckResponseToJson(DnsCheckResponse instance) =>
    <String, dynamic>{'findings': instance.findings};
