// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// How large a heading is.
@JsonEnum()
enum HeadingLevel {
  @JsonValue('h1')
  h1('h1'),
  @JsonValue('h2')
  h2('h2'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const HeadingLevel(this.json);

  factory HeadingLevel.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<HeadingLevel> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
