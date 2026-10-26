// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Where a media asset is in its upload and processing life.
@JsonEnum()
enum MediaStatusDto {
  @JsonValue('uploading')
  uploading('uploading'),
  @JsonValue('processing')
  processing('processing'),
  @JsonValue('ready')
  ready('ready'),
  @JsonValue('failed')
  failed('failed'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const MediaStatusDto(this.json);

  factory MediaStatusDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<MediaStatusDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
