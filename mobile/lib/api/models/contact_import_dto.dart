// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'contact_import_report_dto.dart';
import 'contact_import_status_dto.dart';

part 'contact_import_dto.g.dart';

/// A contact import, with its live counts.
@JsonSerializable()
class ContactImportDto {
  const ContactImportDto({
    required this.attestedAt,
    required this.createdAt,
    required this.id,
    required this.report,
    required this.rowsOk,
    required this.rowsRejected,
    required this.rowsTotal,
    required this.status,
    required this.updatedAt,
    this.listId,
  });
  
  factory ContactImportDto.fromJson(Map<String, Object?> json) => _$ContactImportDtoFromJson(json);
  
  /// When the owner attested consent for the addresses.
  @JsonKey(name: 'attested_at')
  final DateTime attestedAt;

  /// When the import started.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Import ID.
  final String id;

  /// The list the import adds its contacts to, until that list is deleted.
  @JsonKey(name: 'list_id')
  final String? listId;

  /// The rejected rows.
  final ContactImportReportDto report;

  /// How many rows became or updated contacts so far.
  @JsonKey(name: 'rows_ok')
  final int rowsOk;

  /// How many rows were rejected so far.
  @JsonKey(name: 'rows_rejected')
  final int rowsRejected;

  /// How many data rows the file had.
  @JsonKey(name: 'rows_total')
  final int rowsTotal;

  /// Where it is.
  final ContactImportStatusDto status;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$ContactImportDtoToJson(this);
}
