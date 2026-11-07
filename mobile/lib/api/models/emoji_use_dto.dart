// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// How much emoji a draft uses.
@JsonEnum()
enum EmojiUseDto {
  @JsonValue('none')
  none('none'),
  @JsonValue('light')
  light('light'),
  @JsonValue('heavy')
  heavy('heavy'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const EmojiUseDto(this.json);

  factory EmojiUseDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<EmojiUseDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
