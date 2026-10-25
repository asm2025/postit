// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaDto _$MediaDtoFromJson(Map<String, dynamic> json) => MediaDto(
  bytes: (json['bytes'] as num).toInt(),
  createdAt: DateTime.parse(json['created_at'] as String),
  hasThumbnail: json['has_thumbnail'] as bool,
  id: json['id'] as String,
  kind: MediaKindDto.fromJson(json['kind'] as String),
  status: MediaStatusDto.fromJson(json['status'] as String),
  derivedFrom: json['derived_from'] as String?,
  durationMs: (json['duration_ms'] as num?)?.toInt(),
  height: (json['height'] as num?)?.toInt(),
  mime: json['mime'] as String?,
  originalFilename: json['original_filename'] as String?,
  sha256: json['sha256'] as String?,
  width: (json['width'] as num?)?.toInt(),
);

Map<String, dynamic> _$MediaDtoToJson(MediaDto instance) => <String, dynamic>{
  'bytes': instance.bytes,
  'created_at': instance.createdAt.toIso8601String(),
  'derived_from': instance.derivedFrom,
  'duration_ms': instance.durationMs,
  'has_thumbnail': instance.hasThumbnail,
  'height': instance.height,
  'id': instance.id,
  'kind': _$MediaKindDtoEnumMap[instance.kind]!,
  'mime': instance.mime,
  'original_filename': instance.originalFilename,
  'sha256': instance.sha256,
  'status': _$MediaStatusDtoEnumMap[instance.status]!,
  'width': instance.width,
};

const _$MediaKindDtoEnumMap = {
  MediaKindDto.image: 'image',
  MediaKindDto.video: 'video',
  MediaKindDto.$unknown: r'$unknown',
};

const _$MediaStatusDtoEnumMap = {
  MediaStatusDto.uploading: 'uploading',
  MediaStatusDto.processing: 'processing',
  MediaStatusDto.ready: 'ready',
  MediaStatusDto.failed: 'failed',
  MediaStatusDto.$unknown: r'$unknown',
};
