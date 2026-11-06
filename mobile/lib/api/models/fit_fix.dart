// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'aspect_ratio.dart';
import 'fit_fix_type.dart';
import 'suggested_fix.dart';

part 'fit_fix.g.dart';

/// Crop or pad the media to this aspect ratio.
@JsonSerializable()
class FitFix {
  const FitFix({
    required this.aspect,
    required this.type,
  });
  
  factory FitFix.fromJson(Map<String, Object?> json) => _$FitFixFromJson(json);
  
  /// Target ratio.
  final AspectRatio aspect;
  final FitFixType type;

  Map<String, Object?> toJson() => _$FitFixToJson(this);
}
