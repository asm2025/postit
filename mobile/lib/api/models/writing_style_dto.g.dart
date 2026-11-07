// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'writing_style_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

WritingStyleDto _$WritingStyleDtoFromJson(Map<String, dynamic> json) =>
    WritingStyleDto(
      accountId: json['account_id'] as String,
      emoji: EmojiUseDto.fromJson(json['emoji'] as String),
      hashtags: HashtagUseDto.fromJson(json['hashtags'] as String),
      notes: json['notes'] as String,
      tone: json['tone'] as String,
      id: json['id'] as String?,
      language: json['language'] as String?,
      updatedAt: json['updated_at'] == null
          ? null
          : DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$WritingStyleDtoToJson(WritingStyleDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'emoji': _$EmojiUseDtoEnumMap[instance.emoji]!,
      'hashtags': _$HashtagUseDtoEnumMap[instance.hashtags]!,
      'id': instance.id,
      'language': instance.language,
      'notes': instance.notes,
      'tone': instance.tone,
      'updated_at': instance.updatedAt?.toIso8601String(),
    };

const _$EmojiUseDtoEnumMap = {
  EmojiUseDto.none: 'none',
  EmojiUseDto.light: 'light',
  EmojiUseDto.heavy: 'heavy',
  EmojiUseDto.$unknown: r'$unknown',
};

const _$HashtagUseDtoEnumMap = {
  HashtagUseDto.none: 'none',
  HashtagUseDto.few: 'few',
  HashtagUseDto.many: 'many',
  HashtagUseDto.$unknown: r'$unknown',
};
