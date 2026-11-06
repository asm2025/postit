// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'shorten_text_fix_type.dart';
import 'suggested_fix.dart';

part 'shorten_text_fix.g.dart';

/// Shorten the text to at most this many units of the format's measure.
@JsonSerializable()
class ShortenTextFix {
  const ShortenTextFix({
    required this.max,
    required this.type,
  });
  
  factory ShortenTextFix.fromJson(Map<String, Object?> json) => _$ShortenTextFixFromJson(json);
  
  /// Maximum length.
  final int max;
  final ShortenTextFixType type;

  Map<String, Object?> toJson() => _$ShortenTextFixToJson(this);
}
