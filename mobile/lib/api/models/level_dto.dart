// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// What a delegate may do in the owner's workspace.
@JsonEnum()
enum LevelDto {
  @JsonValue('view')
  view('view'),
  @JsonValue('edit')
  edit('edit'),
  @JsonValue('publish')
  publish('publish'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const LevelDto(this.json);

  factory LevelDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<LevelDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
