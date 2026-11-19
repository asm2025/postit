// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'upload_offset_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UploadOffsetDto _$UploadOffsetDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['offset'],
    disallowNullValues: const ['offset'],
  );
  return UploadOffsetDto(
    offset: UploadOffsetDto._read_offset(json['offset']),
    additionalProperties:
        UploadOffsetDto._extras(json, 'additionalProperties') == null
        ? const {}
        : UploadOffsetDto._readExtras(
            UploadOffsetDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$UploadOffsetDtoToJson(UploadOffsetDto instance) =>
    <String, dynamic>{
      'offset': ?UploadOffsetDto._write_offset(instance.offset),
    };
