// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'media_asset_id.dart';

part 'media_item.g.dart';

/// One media asset attached to a post.
@JsonSerializable()
class MediaItem {
  const MediaItem({
    required this.asset,
    this.altText,
  });
  
  factory MediaItem.fromJson(Map<String, Object?> json) => _$MediaItemFromJson(json);
  
  /// Description for readers who cannot see the media.
  @JsonKey(name: 'alt_text')
  final String? altText;

  /// The stored asset.
  final MediaAssetId asset;

  Map<String, Object?> toJson() => _$MediaItemToJson(this);
}
