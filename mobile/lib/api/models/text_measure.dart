// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// How a platform counts the length of a post.
@JsonEnum()
enum TextMeasure {
  @JsonValue('graphemes')
  graphemes('graphemes'),
  @JsonValue('x_weighted')
  xWeighted('x_weighted'),
  @JsonValue('utf16_units')
  utf16Units('utf16_units'),
  @JsonValue('bytes')
  bytes('bytes'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const TextMeasure(this.json);

  factory TextMeasure.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<TextMeasure> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
