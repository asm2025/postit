// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// A kind of post a platform can publish. The set is closed.
@JsonEnum()
enum FormatId {
  @JsonValue('text')
  text('text'),
  @JsonValue('link')
  link('link'),
  @JsonValue('image')
  image('image'),
  @JsonValue('carousel')
  carousel('carousel'),
  @JsonValue('video')
  video('video'),
  @JsonValue('short_video')
  shortVideo('short_video'),
  @JsonValue('story')
  story('story'),
  @JsonValue('reel')
  reel('reel'),
  @JsonValue('html_email')
  htmlEmail('html_email'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const FormatId(this.json);

  factory FormatId.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<FormatId> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
