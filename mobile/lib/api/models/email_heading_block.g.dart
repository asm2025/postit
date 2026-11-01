// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_heading_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailHeadingBlock _$EmailHeadingBlockFromJson(Map<String, dynamic> json) =>
    EmailHeadingBlock(
      level: HeadingLevel.fromJson(json['level'] as String),
      text: json['text'] as String,
      type: EmailHeadingBlockType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$EmailHeadingBlockToJson(EmailHeadingBlock instance) =>
    <String, dynamic>{
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
