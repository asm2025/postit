// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Map<String, dynamic> _$EmailBlockToJson(EmailBlock instance) =>
    <String, dynamic>{};

EmailBlockEmailButtonBlock _$EmailBlockEmailButtonBlockFromJson(
  Map<String, dynamic> json,
) => EmailBlockEmailButtonBlock(
  label: json['label'] as String,
  type: EmailButtonBlockType.fromJson(json['type'] as String),
  url: json['url'] as String,
);

Map<String, dynamic> _$EmailBlockEmailButtonBlockToJson(
  EmailBlockEmailButtonBlock instance,
) => <String, dynamic>{
  'label': instance.label,
  'type': _$EmailButtonBlockTypeEnumMap[instance.type]!,
  'url': instance.url,
};

const _$EmailButtonBlockTypeEnumMap = {
  EmailButtonBlockType.button: 'button',
  EmailButtonBlockType.$unknown: r'$unknown',
};

EmailBlockEmailDividerBlock _$EmailBlockEmailDividerBlockFromJson(
  Map<String, dynamic> json,
) => EmailBlockEmailDividerBlock(
  type: EmailDividerBlockType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$EmailBlockEmailDividerBlockToJson(
  EmailBlockEmailDividerBlock instance,
) => <String, dynamic>{'type': _$EmailDividerBlockTypeEnumMap[instance.type]!};

const _$EmailDividerBlockTypeEnumMap = {
  EmailDividerBlockType.divider: 'divider',
  EmailDividerBlockType.$unknown: r'$unknown',
};

EmailBlockEmailHeadingBlock _$EmailBlockEmailHeadingBlockFromJson(
  Map<String, dynamic> json,
) => EmailBlockEmailHeadingBlock(
  level: HeadingLevel.fromJson(json['level'] as String),
  text: json['text'] as String,
  type: EmailHeadingBlockType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$EmailBlockEmailHeadingBlockToJson(
  EmailBlockEmailHeadingBlock instance,
) => <String, dynamic>{
  'level': _$HeadingLevelEnumMap[instance.level]!,
  'text': instance.text,
  'type': _$EmailHeadingBlockTypeEnumMap[instance.type]!,
};

const _$HeadingLevelEnumMap = {
  HeadingLevel.h1: 'h1',
  HeadingLevel.h2: 'h2',
  HeadingLevel.$unknown: r'$unknown',
};

const _$EmailHeadingBlockTypeEnumMap = {
  EmailHeadingBlockType.heading: 'heading',
  EmailHeadingBlockType.$unknown: r'$unknown',
};

EmailBlockEmailImageBlock _$EmailBlockEmailImageBlockFromJson(
  Map<String, dynamic> json,
) => EmailBlockEmailImageBlock(
  alt: json['alt'] as String,
  asset: json['asset'] as String,
  link: json['link'] as String?,
  type: EmailImageBlockType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$EmailBlockEmailImageBlockToJson(
  EmailBlockEmailImageBlock instance,
) => <String, dynamic>{
  'alt': instance.alt,
  'asset': instance.asset,
  'link': instance.link,
  'type': _$EmailImageBlockTypeEnumMap[instance.type]!,
};

const _$EmailImageBlockTypeEnumMap = {
  EmailImageBlockType.image: 'image',
  EmailImageBlockType.$unknown: r'$unknown',
};

EmailBlockEmailTextBlock _$EmailBlockEmailTextBlockFromJson(
  Map<String, dynamic> json,
) => EmailBlockEmailTextBlock(
  text: json['text'] as String,
  type: EmailTextBlockType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$EmailBlockEmailTextBlockToJson(
  EmailBlockEmailTextBlock instance,
) => <String, dynamic>{
  'text': instance.text,
  'type': _$EmailTextBlockTypeEnumMap[instance.type]!,
};

const _$EmailTextBlockTypeEnumMap = {
  EmailTextBlockType.text: 'text',
  EmailTextBlockType.$unknown: r'$unknown',
};
