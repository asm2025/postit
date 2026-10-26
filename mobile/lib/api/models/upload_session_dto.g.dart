// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'upload_session_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UploadSessionDto _$UploadSessionDtoFromJson(Map<String, dynamic> json) =>
    UploadSessionDto(
      chunkMax: (json['chunk_max'] as num).toInt(),
      id: json['id'] as String,
      offset: (json['offset'] as num).toInt(),
      size: (json['size'] as num).toInt(),
      expiresAt: json['expires_at'] == null
          ? null
          : DateTime.parse(json['expires_at'] as String),
    );

Map<String, dynamic> _$UploadSessionDtoToJson(UploadSessionDto instance) =>
    <String, dynamic>{
      'chunk_max': instance.chunkMax,
      'expires_at': instance.expiresAt?.toIso8601String(),
      'id': instance.id,
      'offset': instance.offset,
      'size': instance.size,
    };
