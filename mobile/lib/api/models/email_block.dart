// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_button_block.dart';
import 'email_button_block_type.dart';
import 'email_divider_block.dart';
import 'email_divider_block_type.dart';
import 'email_heading_block.dart';
import 'email_heading_block_type.dart';
import 'email_image_block.dart';
import 'email_image_block_type.dart';
import 'email_text_block.dart';
import 'email_text_block_type.dart';
import 'heading_level.dart';
import 'media_asset_id.dart';


part 'email_block.g.dart';

/// One block of an email body.
@JsonSerializable(createFactory: false)
sealed class EmailBlock {
  const EmailBlock();
  
  factory EmailBlock.fromJson(Map<String, dynamic> json) =>
      EmailBlockSealedDeserializer.tryDeserialize(json);
  
  Map<String, dynamic> toJson();
}

extension EmailBlockSealedDeserializer on EmailBlock {
  static EmailBlock tryDeserialize(
    Map<String, dynamic> json, {
    String key = 'type',
    Map<Type, Object?>? mapping,
  }) {
    final mappingFallback = const <Type, Object?>{
      EmailBlockEmailButtonBlock: 'button',
      EmailBlockEmailDividerBlock: 'divider',
      EmailBlockEmailHeadingBlock: 'heading',
      EmailBlockEmailImageBlock: 'image',
      EmailBlockEmailTextBlock: 'text',
    };
    final value = json[key];
    final effective = mapping ?? mappingFallback;
    return switch (value) {
      _ when value == effective[EmailBlockEmailButtonBlock] => EmailBlockEmailButtonBlock.fromJson(json),
      _ when value == effective[EmailBlockEmailDividerBlock] => EmailBlockEmailDividerBlock.fromJson(json),
      _ when value == effective[EmailBlockEmailHeadingBlock] => EmailBlockEmailHeadingBlock.fromJson(json),
      _ when value == effective[EmailBlockEmailImageBlock] => EmailBlockEmailImageBlock.fromJson(json),
      _ when value == effective[EmailBlockEmailTextBlock] => EmailBlockEmailTextBlock.fromJson(json),
      _ => throw FormatException('Unknown discriminator value "${json[key]}" for EmailBlock'),
    };
  }
}

@JsonSerializable()
class EmailBlockEmailButtonBlock extends EmailBlock implements EmailButtonBlock {
  @override
  final String label;
  @override
  final EmailButtonBlockType type;
  @override
  final String url;

  const EmailBlockEmailButtonBlock({
    required this.label,
    required this.type,
    required this.url,
  });
  
  factory EmailBlockEmailButtonBlock.fromJson(Map<String, dynamic> json) =>
      _$EmailBlockEmailButtonBlockFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$EmailBlockEmailButtonBlockToJson(this);
}
@JsonSerializable()
class EmailBlockEmailDividerBlock extends EmailBlock implements EmailDividerBlock {
  @override
  final EmailDividerBlockType type;

  const EmailBlockEmailDividerBlock({
    required this.type,
  });
  
  factory EmailBlockEmailDividerBlock.fromJson(Map<String, dynamic> json) =>
      _$EmailBlockEmailDividerBlockFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$EmailBlockEmailDividerBlockToJson(this);
}
@JsonSerializable()
class EmailBlockEmailHeadingBlock extends EmailBlock implements EmailHeadingBlock {
  @override
  final HeadingLevel level;
  @override
  final String text;
  @override
  final EmailHeadingBlockType type;

  const EmailBlockEmailHeadingBlock({
    required this.level,
    required this.text,
    required this.type,
  });
  
  factory EmailBlockEmailHeadingBlock.fromJson(Map<String, dynamic> json) =>
      _$EmailBlockEmailHeadingBlockFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$EmailBlockEmailHeadingBlockToJson(this);
}
@JsonSerializable()
class EmailBlockEmailImageBlock extends EmailBlock implements EmailImageBlock {
  @override
  final String alt;
  @override
  final MediaAssetId asset;
  @override
  final String? link;
  @override
  final EmailImageBlockType type;

  const EmailBlockEmailImageBlock({
    required this.alt,
    required this.asset,
    required this.link,
    required this.type,
  });
  
  factory EmailBlockEmailImageBlock.fromJson(Map<String, dynamic> json) =>
      _$EmailBlockEmailImageBlockFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$EmailBlockEmailImageBlockToJson(this);
}
@JsonSerializable()
class EmailBlockEmailTextBlock extends EmailBlock implements EmailTextBlock {
  @override
  final String text;
  @override
  final EmailTextBlockType type;

  const EmailBlockEmailTextBlock({
    required this.text,
    required this.type,
  });
  
  factory EmailBlockEmailTextBlock.fromJson(Map<String, dynamic> json) =>
      _$EmailBlockEmailTextBlockFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$EmailBlockEmailTextBlockToJson(this);
}
