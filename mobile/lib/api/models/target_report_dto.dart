// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'format_id.dart';
import 'violation.dart';

part 'target_report_dto.g.dart';

/// What validating one target found.
@JsonSerializable()
class TargetReportDto {
  const TargetReportDto({
    required this.accountId,
    required this.violations,
    this.format,
  });
  
  factory TargetReportDto.fromJson(Map<String, Object?> json) => _$TargetReportDtoFromJson(json);
  
  /// The account.
  @JsonKey(name: 'account_id')
  final String accountId;

  /// The format the content would go out as, when one fits.
  final FormatId? format;

  /// The rules the content breaks for this account.
  final List<Violation> violations;

  Map<String, Object?> toJson() => _$TargetReportDtoToJson(this);
}
