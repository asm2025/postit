// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_text_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailTextBlock _$EmailTextBlockFromJson(Map<String, dynamic> json) =>
    EmailTextBlock(
      text: json['text'] as String,
      type: EmailTextBlockType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$EmailTextBlockToJson(EmailTextBlock instance) =>
    <String, dynamic>{
      'text': instance.text,
      'type': _$EmailTextBlockTypeEnumMap[instance.type]!,
    };

const _$EmailTextBlockTypeEnumMap = {
  EmailTextBlockType.text: 'text',
  EmailTextBlockType.$unknown: r'$unknown',
};
