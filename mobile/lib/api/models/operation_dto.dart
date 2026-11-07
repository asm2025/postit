// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// What a delivery does on its platform.
@JsonEnum()
enum OperationDto {
  @JsonValue('publish')
  publish('publish'),
  @JsonValue('comment')
  comment('comment'),
  @JsonValue('direct')
  direct('direct'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const OperationDto(this.json);

  factory OperationDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<OperationDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
