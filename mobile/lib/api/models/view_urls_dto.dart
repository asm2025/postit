// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'view_urls_dto.g.dart';

/// Short-lived signed URLs for showing an asset.
@JsonSerializable()
class ViewUrlsDto {
  const ViewUrlsDto({
    required this.contentUrl,
    required this.expiresAt,
    this.thumbnailUrl,
  });
  
  factory ViewUrlsDto.fromJson(Map<String, Object?> json) => _$ViewUrlsDtoFromJson(json);
  
  /// The file.
  @JsonKey(name: 'content_url')
  final String contentUrl;

  /// When both stop working.
  @JsonKey(name: 'expires_at')
  final DateTime expiresAt;

  /// Its thumbnail, when it has one.
  @JsonKey(name: 'thumbnail_url')
  final String? thumbnailUrl;

  Map<String, Object?> toJson() => _$ViewUrlsDtoToJson(this);
}
