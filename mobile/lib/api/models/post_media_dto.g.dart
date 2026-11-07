// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostMediaDto _$PostMediaDtoFromJson(Map<String, dynamic> json) => PostMediaDto(
  mediaId: json['media_id'] as String,
  altText: json['alt_text'] as String?,
  position: (json['position'] as num?)?.toInt(),
);

Map<String, dynamic> _$PostMediaDtoToJson(PostMediaDto instance) =>
    <String, dynamic>{
      'alt_text': instance.altText,
      'media_id': instance.mediaId,
      'position': instance.position,
    };
