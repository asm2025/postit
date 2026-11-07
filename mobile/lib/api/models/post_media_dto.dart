// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'post_media_dto.g.dart';

/// One attachment of a post.
@JsonSerializable()
class PostMediaDto {
  const PostMediaDto({
    required this.mediaId,
    this.altText,
    this.position,
  });
  
  factory PostMediaDto.fromJson(Map<String, Object?> json) => _$PostMediaDtoFromJson(json);
  
  /// Alternative text.
  @JsonKey(name: 'alt_text')
  final String? altText;

  /// The media asset.
  @JsonKey(name: 'media_id')
  final String mediaId;

  /// Its place among the attachments, counting from 0.
  final int? position;

  Map<String, Object?> toJson() => _$PostMediaDtoToJson(this);
}
