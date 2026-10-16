// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// A user's account status.
@JsonEnum()
enum StatusDto {
  @JsonValue('pending')
  pending('pending'),
  @JsonValue('active')
  active('active'),
  @JsonValue('disabled')
  disabled('disabled'),
  @JsonValue('deleting')
  deleting('deleting'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const StatusDto(this.json);

  factory StatusDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<StatusDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
