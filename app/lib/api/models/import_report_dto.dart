// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'import_row_dto.dart';

part 'import_report_dto.g.dart';

/// The result of an import, in file order.
@JsonSerializable()
class ImportReportDto {
  const ImportReportDto({
    required this.commit,
    required this.counts,
    required this.rows,
  });
  
  factory ImportReportDto.fromJson(Map<String, Object?> json) => _$ImportReportDtoFromJson(json);
  
  /// Whether accounts were connected (`true`) or the file was only checked.
  final bool commit;

  /// How many rows ended each way, keyed by status.
  final Map<String, int> counts;

  /// One entry per data row.
  final List<ImportRowDto> rows;

  Map<String, Object?> toJson() => _$ImportReportDtoToJson(this);
}
