// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_divider_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailDividerBlock _$EmailDividerBlockFromJson(Map<String, dynamic> json) =>
    EmailDividerBlock(
      type: EmailDividerBlockType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$EmailDividerBlockToJson(EmailDividerBlock instance) =>
    <String, dynamic>{'type': _$EmailDividerBlockTypeEnumMap[instance.type]!};

const _$EmailDividerBlockTypeEnumMap = {
  EmailDividerBlockType.divider: 'divider',
  EmailDividerBlockType.$unknown: r'$unknown',
};
