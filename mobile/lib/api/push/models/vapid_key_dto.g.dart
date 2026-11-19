// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'vapid_key_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

VapidKeyDto _$VapidKeyDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['key'],
    disallowNullValues: const ['key'],
  );
  return VapidKeyDto(
    key: json['key'] as String,
    additionalProperties:
        VapidKeyDto._extras(json, 'additionalProperties') == null
        ? const {}
        : VapidKeyDto._readExtras(
            VapidKeyDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$VapidKeyDtoToJson(VapidKeyDto instance) =>
    <String, dynamic>{'key': instance.key};
