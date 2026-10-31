// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'domain_record.dart';
import 'domain_status.dart';

part 'domain_finding.g.dart';

/// One result of a sender-domain check. Clients translate it by `code`.
@JsonSerializable()
class DomainFinding {
  const DomainFinding({
    required this.code,
    required this.record,
    required this.status,
  });
  
  factory DomainFinding.fromJson(Map<String, Object?> json) => _$DomainFindingFromJson(json);
  
  /// A short fixed code naming the finding, such as `spf_missing`.
  final String code;

  /// Which record.
  final DomainRecord record;

  /// How it looks.
  final DomainStatus status;

  Map<String, Object?> toJson() => _$DomainFindingToJson(this);
}
