// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'unread_count_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UnreadCountDto _$UnreadCountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['count'],
    disallowNullValues: const ['count'],
  );
  return UnreadCountDto(
    count: UnreadCountDto._read_count(json['count']),
    additionalProperties:
        UnreadCountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : UnreadCountDto._readExtras(
            UnreadCountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$UnreadCountDtoToJson(UnreadCountDto instance) =>
    <String, dynamic>{'count': ?UnreadCountDto._write_count(instance.count)};
