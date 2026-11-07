// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Where a post is in its publishing life.
@JsonEnum()
enum PostStatusDto {
  @JsonValue('draft')
  draft('draft'),
  @JsonValue('scheduled')
  scheduled('scheduled'),
  @JsonValue('publishing')
  publishing('publishing'),
  @JsonValue('published')
  published('published'),
  @JsonValue('partial')
  partial('partial'),
  @JsonValue('failed')
  failed('failed'),
  @JsonValue('cancelled')
  cancelled('cancelled'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const PostStatusDto(this.json);

  factory PostStatusDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<PostStatusDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
