// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_image_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailImageBlock _$EmailImageBlockFromJson(Map<String, dynamic> json) =>
    EmailImageBlock(
      alt: json['alt'] as String,
      asset: json['asset'] as String,
      type: EmailImageBlockType.fromJson(json['type'] as String),
      link: json['link'] as String?,
    );

Map<String, dynamic> _$EmailImageBlockToJson(EmailImageBlock instance) =>
    <String, dynamic>{
      'alt': instance.alt,
      'asset': instance.asset,
      'link': instance.link,
      'type': _$EmailImageBlockTypeEnumMap[instance.type]!,
    };

const _$EmailImageBlockTypeEnumMap = {
  EmailImageBlockType.image: 'image',
  EmailImageBlockType.$unknown: r'$unknown',
};
