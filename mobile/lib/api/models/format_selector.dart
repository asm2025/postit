// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'format_selector.g.dart';

/// Picks a format explicitly: the format applies when the target's option equals the value.
@JsonSerializable()
class FormatSelector {
  const FormatSelector({
    required this.option,
    required this.value,
  });
  
  factory FormatSelector.fromJson(Map<String, Object?> json) => _$FormatSelectorFromJson(json);
  
  /// Name of the option in the agent's options schema.
  final String option;

  /// The option value that selects this format.
  final String value;

  Map<String, Object?> toJson() => _$FormatSelectorToJson(this);
}
