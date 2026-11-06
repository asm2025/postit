// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'suggested_fix.dart';
import 'trim_fix_type.dart';

part 'trim_fix.g.dart';

/// Trim the video to at most this many milliseconds.
@JsonSerializable()
class TrimFix {
  const TrimFix({
    required this.maxMs,
    required this.type,
  });
  
  factory TrimFix.fromJson(Map<String, Object?> json) => _$TrimFixFromJson(json);
  
  /// Maximum duration in milliseconds.
  @JsonKey(name: 'max_ms')
  final int maxMs;
  final TrimFixType type;

  Map<String, Object?> toJson() => _$TrimFixToJson(this);
}
