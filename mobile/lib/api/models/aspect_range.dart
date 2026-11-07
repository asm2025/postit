// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'aspect_ratio.dart';

part 'aspect_range.g.dart';

/// A range of acceptable aspect ratios.
@JsonSerializable()
class AspectRange {
  const AspectRange({
    required this.max,
    required this.min,
    required this.preferred,
  });
  
  factory AspectRange.fromJson(Map<String, Object?> json) => _$AspectRangeFromJson(json);
  
  /// Widest ratio allowed.
  final AspectRatio max;

  /// Narrowest ratio (width over height) allowed.
  final AspectRatio min;

  /// The ratio a fit should aim for.
  final AspectRatio preferred;

  Map<String, Object?> toJson() => _$AspectRangeToJson(this);
}
