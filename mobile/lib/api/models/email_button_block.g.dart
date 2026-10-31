// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_button_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailButtonBlock _$EmailButtonBlockFromJson(Map<String, dynamic> json) =>
    EmailButtonBlock(
      label: json['label'] as String,
      type: EmailButtonBlockType.fromJson(json['type'] as String),
      url: json['url'] as String,
    );

Map<String, dynamic> _$EmailButtonBlockToJson(EmailButtonBlock instance) =>
    <String, dynamic>{
      'label': instance.label,
      'type': _$EmailButtonBlockTypeEnumMap[instance.type]!,
      'url': instance.url,
    };

const _$EmailButtonBlockTypeEnumMap = {
  EmailButtonBlockType.button: 'button',
  EmailButtonBlockType.$unknown: r'$unknown',
};
