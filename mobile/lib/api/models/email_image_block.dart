// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_image_block_type.dart';
import 'media_asset_id.dart';

part 'email_image_block.g.dart';

/// An image from the media library.
@JsonSerializable()
class EmailImageBlock {
  const EmailImageBlock({
    required this.alt,
    required this.asset,
    required this.type,
    this.link,
  });
  
  factory EmailImageBlock.fromJson(Map<String, Object?> json) => _$EmailImageBlockFromJson(json);
  
  /// Text for readers who cannot see the image.
  final String alt;

  /// The stored asset.
  final MediaAssetId asset;

  /// Where the image links to, if anywhere.
  final String? link;
  final EmailImageBlockType type;

  Map<String, Object?> toJson() => _$EmailImageBlockToJson(this);
}
