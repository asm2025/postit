// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'column_map.g.dart';

/// Which CSV header holds each contact field. Header names are matched after trimming.
@JsonSerializable()
class ColumnMap {
  const ColumnMap({
    required this.consentSource,
    required this.email,
    this.consentAt,
    this.fields,
    this.firstName,
    this.lastName,
    this.locale,
    this.tags,
  });
  
  factory ColumnMap.fromJson(Map<String, Object?> json) => _$ColumnMapFromJson(json);
  
  /// When consent was given: RFC 3339, or a `YYYY-MM-DD` date taken as midnight UTC.
  @JsonKey(name: 'consent_at')
  final String? consentAt;

  /// The column noting where consent came from. Required.
  @JsonKey(name: 'consent_source')
  final String consentSource;

  /// The address column. Required.
  final String email;

  /// Custom merge fields: CSV header to field name.
  final Map<String, String>? fields;

  /// Given name.
  @JsonKey(name: 'first_name')
  final String? firstName;

  /// Family name.
  @JsonKey(name: 'last_name')
  final String? lastName;

  /// The contact's language.
  final String? locale;

  /// Tags, separated by `|` in a cell.
  final String? tags;

  Map<String, Object?> toJson() => _$ColumnMapToJson(this);
}
