// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'format_id.dart';
import 'format_selector.dart';
import 'media_rules.dart';
import 'text_rules.dart';

part 'format_spec.g.dart';

/// One format an agent can publish.
@JsonSerializable()
class FormatSpec {
  const FormatSpec({
    required this.id,
    required this.media,
    required this.text,
    this.selectedBy,
  });
  
  factory FormatSpec.fromJson(Map<String, Object?> json) => _$FormatSpecFromJson(json);
  
  /// Which format this is.
  final FormatId id;

  /// Media limits.
  final MediaRules media;

  /// Option that selects this format over content-based inference.
  @JsonKey(name: 'selected_by')
  final FormatSelector? selectedBy;

  /// Text limits.
  final TextRules text;

  Map<String, Object?> toJson() => _$FormatSpecToJson(this);
}
