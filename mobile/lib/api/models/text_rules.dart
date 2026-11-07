// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'text_measure.dart';

part 'text_rules.g.dart';

/// Limits on the text of a format.
@JsonSerializable()
class TextRules {
  const TextRules({
    required this.max,
    required this.measure,
    required this.requiredValue,
    this.includesLinkAndTags,
  });
  
  factory TextRules.fromJson(Map<String, Object?> json) => _$TextRulesFromJson(json);
  
  /// Whether the platform publishes the text, the link, and the `#tags` as one string, so.
  /// the length is measured on [`post_text`](crate::post_text). Absent means `false`.
  @JsonKey(name: 'includes_link_and_tags')
  final bool? includesLinkAndTags;

  /// Longest text allowed, in `measure` units.
  final int max;

  /// How the platform counts length.
  final TextMeasure measure;

  /// Whether the text may not be empty.
  /// The name has been replaced because it contains a keyword. Original name: `required`.
  @JsonKey(name: 'required')
  final bool requiredValue;

  Map<String, Object?> toJson() => _$TextRulesToJson(this);
}
