// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'view_urls_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ViewUrlsDto _$ViewUrlsDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['content_url', 'expires_at'],
    disallowNullValues: const ['content_url', 'expires_at'],
  );
  return ViewUrlsDto(
    contentUrl: json['content_url'] as String,
    expiresAt: ViewUrlsDto._read_expiresAt(json['expires_at']),
    thumbnailUrl: json['thumbnail_url'] as String?,
    additionalProperties:
        ViewUrlsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ViewUrlsDto._readExtras(
            ViewUrlsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ViewUrlsDtoToJson(ViewUrlsDto instance) =>
    <String, dynamic>{
      'content_url': instance.contentUrl,
      'expires_at': ?ViewUrlsDto._write_expiresAt(instance.expiresAt),
      'thumbnail_url': ?instance.thumbnailUrl,
    };
