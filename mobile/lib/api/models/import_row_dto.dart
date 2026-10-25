// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'import_status_dto.dart';

part 'import_row_dto.g.dart';

/// The report line for one data row. Never carries a field value.
@JsonSerializable()
class ImportRowDto {
  const ImportRowDto({
    required this.accountIds,
    required this.displayNameIgnored,
    required this.row,
    required this.status,
    required this.violations,
    this.field,
  });
  
  factory ImportRowDto.fromJson(Map<String, Object?> json) => _$ImportRowDtoFromJson(json);
  
  /// The accounts created or matched; empty on a dry run.
  @JsonKey(name: 'account_ids')
  final List<String> accountIds;

  /// A `display_name` was given but the row reaches more than one account.
  @JsonKey(name: 'display_name_ignored')
  final bool displayNameIgnored;

  /// The credential field at fault, when known.
  final String? field;

  /// The 1-based data row number.
  final int row;

  /// How the row ended.
  final ImportStatusDto status;

  /// The violation codes, for `invalid_fields`.
  final List<String> violations;

  Map<String, Object?> toJson() => _$ImportRowDtoToJson(this);
}
