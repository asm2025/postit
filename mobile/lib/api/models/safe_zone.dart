// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'message.dart';

part 'safe_zone.g.dart';

/// A part of the frame the platform's interface covers. Fractions of the frame, 0 to 1.
@JsonSerializable()
class SafeZone {
  const SafeZone({
    required this.height,
    required this.label,
    required this.width,
    required this.x,
    required this.y,
  });
  
  factory SafeZone.fromJson(Map<String, Object?> json) => _$SafeZoneFromJson(json);
  
  /// Height.
  final double height;

  /// What the zone is for, e.g. "keep captions inside".
  final Message label;

  /// Width.
  final double width;

  /// Left edge.
  final double x;

  /// Top edge.
  final double y;

  Map<String, Object?> toJson() => _$SafeZoneToJson(this);
}
