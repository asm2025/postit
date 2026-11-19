// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'calendar_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CalendarDto _$CalendarDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['items'],
    disallowNullValues: const ['items'],
  );
  return CalendarDto(
    items: (json['items'] as List<dynamic>)
        .map((e) => CalendarItemDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        CalendarDto._extras(json, 'additionalProperties') == null
        ? const {}
        : CalendarDto._readExtras(
            CalendarDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CalendarDtoToJson(CalendarDto instance) =>
    <String, dynamic>{'items': instance.items.map((e) => e.toJson()).toList()};
