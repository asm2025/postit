// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'native_schedule_window.g.dart';

/// How far ahead a platform accepts a natively scheduled post.
@JsonSerializable()
class NativeScheduleWindow {
  const NativeScheduleWindow({
    required this.maxLeadSecs,
    required this.minLeadSecs,
  });
  
  factory NativeScheduleWindow.fromJson(Map<String, Object?> json) => _$NativeScheduleWindowFromJson(json);
  
  /// Longest lead time, in seconds.
  @JsonKey(name: 'max_lead_secs')
  final int maxLeadSecs;

  /// Shortest lead time, in seconds.
  @JsonKey(name: 'min_lead_secs')
  final int minLeadSecs;

  Map<String, Object?> toJson() => _$NativeScheduleWindowToJson(this);
}
