// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why a delegation ended.
@JsonEnum()
enum EndedReasonDto {
  @JsonValue('revoked')
  revoked('revoked'),
  @JsonValue('left')
  left('left'),
  @JsonValue('expired')
  expired('expired'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const EndedReasonDto(this.json);

  factory EndedReasonDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<EndedReasonDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
