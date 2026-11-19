// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostMediaDto _$PostMediaDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['media_id'],
    disallowNullValues: const ['media_id', 'position'],
  );
  return PostMediaDto(
    altText: json['alt_text'] as String?,
    mediaId: json['media_id'] as String,
    position: PostMediaDto._read_position(json['position']),
    additionalProperties:
        PostMediaDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PostMediaDto._readExtras(
            PostMediaDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PostMediaDtoToJson(PostMediaDto instance) =>
    <String, dynamic>{
      'alt_text': ?instance.altText,
      'media_id': instance.mediaId,
      'position': ?PostMediaDto._write_position(instance.position),
    };
