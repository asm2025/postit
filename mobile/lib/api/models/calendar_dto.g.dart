// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'calendar_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CalendarDto _$CalendarDtoFromJson(Map<String, dynamic> json) => CalendarDto(
  items: (json['items'] as List<dynamic>)
      .map((e) => CalendarItemDto.fromJson(e as Map<String, dynamic>))
      .toList(),
);

Map<String, dynamic> _$CalendarDtoToJson(CalendarDto instance) =>
    <String, dynamic>{'items': instance.items};
