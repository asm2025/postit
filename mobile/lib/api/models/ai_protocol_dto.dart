// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// The wire protocol a provider speaks.
@JsonEnum()
enum AiProtocolDto {
  @JsonValue('openai-compatible')
  openaiCompatible('openai-compatible'),
  @JsonValue('anthropic')
  anthropic('anthropic'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const AiProtocolDto(this.json);

  factory AiProtocolDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<AiProtocolDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
