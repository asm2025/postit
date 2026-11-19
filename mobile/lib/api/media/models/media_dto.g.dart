// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaDto _$MediaDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'bytes',
      'created_at',
      'has_thumbnail',
      'id',
      'kind',
      'status',
    ],
    disallowNullValues: const [
      'bytes',
      'created_at',
      'has_thumbnail',
      'id',
      'kind',
      'status',
    ],
  );
  return MediaDto(
    bytes: MediaDto._read_bytes(json['bytes']),
    createdAt: MediaDto._read_createdAt(json['created_at']),
    derivedFrom: json['derived_from'] as String?,
    durationMs: MediaDto._read_durationMs(json['duration_ms']),
    hasThumbnail: json['has_thumbnail'] as bool,
    height: MediaDto._read_height(json['height']),
    id: json['id'] as String,
    kind: MediaDto._read_kind(json['kind']),
    mime: json['mime'] as String?,
    originalFilename: json['original_filename'] as String?,
    sha256: json['sha256'] as String?,
    status: MediaDto._read_status(json['status']),
    width: MediaDto._read_width(json['width']),
    additionalProperties: MediaDto._extras(json, 'additionalProperties') == null
        ? const {}
        : MediaDto._readExtras(MediaDto._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$MediaDtoToJson(MediaDto instance) => <String, dynamic>{
  'bytes': ?MediaDto._write_bytes(instance.bytes),
  'created_at': ?MediaDto._write_createdAt(instance.createdAt),
  'derived_from': ?instance.derivedFrom,
  'duration_ms': ?MediaDto._write_durationMs(instance.durationMs),
  'has_thumbnail': instance.hasThumbnail,
  'height': ?MediaDto._write_height(instance.height),
  'id': instance.id,
  'kind': ?MediaDto._write_kind(instance.kind),
  'mime': ?instance.mime,
  'original_filename': ?instance.originalFilename,
  'sha256': ?instance.sha256,
  'status': ?MediaDto._write_status(instance.status),
  'width': ?MediaDto._write_width(instance.width),
};
