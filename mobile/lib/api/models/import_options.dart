// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'column_map.dart';
import 'contact_list_id.dart';

part 'import_options.g.dart';

/// What an import is told before the file is read.
@JsonSerializable()
class ImportOptions {
  const ImportOptions({
    required this.attestConsent,
    required this.columns,
    this.listId,
  });
  
  factory ImportOptions.fromJson(Map<String, Object?> json) => _$ImportOptionsFromJson(json);
  
  /// The owner attests they have consent to email these addresses. Must be `true`.
  @JsonKey(name: 'attest_consent')
  final bool attestConsent;

  /// Which header holds which field.
  final ColumnMap columns;

  /// The list the import adds its contacts to.
  @JsonKey(name: 'list_id')
  final ContactListId? listId;

  Map<String, Object?> toJson() => _$ImportOptionsToJson(this);
}
