// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'view_urls_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ViewUrlsDto _$ViewUrlsDtoFromJson(Map<String, dynamic> json) => ViewUrlsDto(
  contentUrl: json['content_url'] as String,
  expiresAt: DateTime.parse(json['expires_at'] as String),
  thumbnailUrl: json['thumbnail_url'] as String?,
);

Map<String, dynamic> _$ViewUrlsDtoToJson(ViewUrlsDto instance) =>
    <String, dynamic>{
      'content_url': instance.contentUrl,
      'expires_at': instance.expiresAt.toIso8601String(),
      'thumbnail_url': instance.thumbnailUrl,
    };
