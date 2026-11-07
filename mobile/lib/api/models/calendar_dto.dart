// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'calendar_item_dto.dart';

part 'calendar_dto.g.dart';

/// The calendar of a range.
@JsonSerializable()
class CalendarDto {
  const CalendarDto({
    required this.items,
  });
  
  factory CalendarDto.fromJson(Map<String, Object?> json) => _$CalendarDtoFromJson(json);
  
  /// Live jobs due in the range, soonest first.
  final List<CalendarItemDto> items;

  Map<String, Object?> toJson() => _$CalendarDtoToJson(this);
}
