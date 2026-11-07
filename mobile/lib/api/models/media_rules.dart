// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'aspect_range.dart';
import 'media_kind.dart';
import 'safe_zone.dart';

part 'media_rules.g.dart';

/// Limits on the media of a format.
@JsonSerializable()
class MediaRules {
  const MediaRules({
    required this.aspectRatios,
    required this.kinds,
    required this.maxCount,
    required this.mimeTypes,
    required this.minCount,
    required this.safeZones,
    this.maxBytes,
    this.maxDurationMs,
    this.maxHeight,
    this.maxWidth,
    this.minDurationMs,
    this.minHeight,
    this.minWidth,
  });
  
  factory MediaRules.fromJson(Map<String, Object?> json) => _$MediaRulesFromJson(json);
  
  /// Allowed aspect ratios; empty allows any.
  @JsonKey(name: 'aspect_ratios')
  final List<AspectRange> aspectRatios;

  /// Allowed media kinds.
  final List<MediaKind> kinds;

  /// Largest file size per item.
  @JsonKey(name: 'max_bytes')
  final int? maxBytes;

  /// Most items allowed.
  @JsonKey(name: 'max_count')
  final int maxCount;

  /// Longest duration in milliseconds.
  @JsonKey(name: 'max_duration_ms')
  final int? maxDurationMs;

  /// Tallest height in pixels.
  @JsonKey(name: 'max_height')
  final int? maxHeight;

  /// Widest width in pixels.
  @JsonKey(name: 'max_width')
  final int? maxWidth;

  /// Allowed MIME types; empty allows any of the kinds.
  @JsonKey(name: 'mime_types')
  final List<String> mimeTypes;

  /// Fewest items allowed.
  @JsonKey(name: 'min_count')
  final int minCount;

  /// Shortest duration in milliseconds.
  @JsonKey(name: 'min_duration_ms')
  final int? minDurationMs;

  /// Shortest height in pixels.
  @JsonKey(name: 'min_height')
  final int? minHeight;

  /// Narrowest width in pixels.
  @JsonKey(name: 'min_width')
  final int? minWidth;

  /// Regions the interface covers.
  @JsonKey(name: 'safe_zones')
  final List<SafeZone> safeZones;

  Map<String, Object?> toJson() => _$MediaRulesToJson(this);
}
