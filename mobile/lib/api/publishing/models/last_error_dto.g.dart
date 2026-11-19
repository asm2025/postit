// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'last_error_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LastErrorDto _$LastErrorDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['class', 'code'],
    disallowNullValues: const ['class', 'code'],
  );
  return LastErrorDto(
    class2: json['class'] as String,
    code: json['code'] as String,
    additionalProperties:
        LastErrorDto._extras(json, 'additionalProperties') == null
        ? const {}
        : LastErrorDto._readExtras(
            LastErrorDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$LastErrorDtoToJson(LastErrorDto instance) =>
    <String, dynamic>{'class': instance.class2, 'code': instance.code};
