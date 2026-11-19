// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'upload_session_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UploadSessionDto _$UploadSessionDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['chunk_max', 'id', 'offset', 'size'],
    disallowNullValues: const ['chunk_max', 'id', 'offset', 'size'],
  );
  return UploadSessionDto(
    chunkMax: UploadSessionDto._read_chunkMax(json['chunk_max']),
    expiresAt: UploadSessionDto._read_expiresAt(json['expires_at']),
    id: json['id'] as String,
    offset: UploadSessionDto._read_offset(json['offset']),
    size: UploadSessionDto._read_size(json['size']),
    additionalProperties:
        UploadSessionDto._extras(json, 'additionalProperties') == null
        ? const {}
        : UploadSessionDto._readExtras(
            UploadSessionDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$UploadSessionDtoToJson(UploadSessionDto instance) =>
    <String, dynamic>{
      'chunk_max': ?UploadSessionDto._write_chunkMax(instance.chunkMax),
      'expires_at': ?UploadSessionDto._write_expiresAt(instance.expiresAt),
      'id': instance.id,
      'offset': ?UploadSessionDto._write_offset(instance.offset),
      'size': ?UploadSessionDto._write_size(instance.size),
    };
