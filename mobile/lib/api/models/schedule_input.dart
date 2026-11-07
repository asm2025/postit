// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'schedule_input.g.dart';

/// When a post goes out, as a client sends it.
///
/// A string is RFC 3339 *with* an offset (`2026-11-08T09:00:00+02:00`); a string without an.
/// offset does not parse. An object is a local date-time (seconds optional) and an optional.
/// IANA zone; without a zone the caller's default applies.
@JsonSerializable()
class ScheduleInput {
  const ScheduleInput();
  
  factory ScheduleInput.fromJson(Map<String, Object?> json) => _$ScheduleInputFromJson(json);
  
  Map<String, Object?> toJson() => _$ScheduleInputToJson(this);
}
