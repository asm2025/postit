// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'domain_finding.dart';

part 'dns_check_response.g.dart';

/// What the sending domain's DNS records look like. Each finding is a fixed code the clients.
/// translate; no domain, selector, or record text is returned.
@JsonSerializable()
class DnsCheckResponse {
  const DnsCheckResponse({
    required this.findings,
  });
  
  factory DnsCheckResponse.fromJson(Map<String, Object?> json) => _$DnsCheckResponseFromJson(json);
  
  /// One finding each for SPF, DKIM, and DMARC, in that order.
  final List<DomainFinding> findings;

  Map<String, Object?> toJson() => _$DnsCheckResponseToJson(this);
}
