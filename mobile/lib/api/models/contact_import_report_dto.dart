// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'contact_import_rejection_dto.dart';

part 'contact_import_report_dto.g.dart';

/// Which rows were rejected and why. Row numbers and codes only, never an address.
@JsonSerializable()
class ContactImportReportDto {
  const ContactImportReportDto({
    this.entries,
    this.totals,
  });
  
  factory ContactImportReportDto.fromJson(Map<String, Object?> json) => _$ContactImportReportDtoFromJson(json);
  
  /// The first 1,000 rejected rows.
  final List<ContactImportRejectionDto>? entries;

  /// Every reject, counted by code.
  final Map<String, int>? totals;

  Map<String, Object?> toJson() => _$ContactImportReportDtoToJson(this);
}
