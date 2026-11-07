// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_writing_style_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutWritingStyleRequest _$PutWritingStyleRequestFromJson(
  Map<String, dynamic> json,
) => PutWritingStyleRequest(
  emoji: json['emoji'] == null
      ? null
      : EmojiUseDto.fromJson(json['emoji'] as String),
  hashtags: json['hashtags'] == null
      ? null
      : HashtagUseDto.fromJson(json['hashtags'] as String),
  language: json['language'] as String?,
  notes: json['notes'] as String?,
  tone: json['tone'] as String?,
);

Map<String, dynamic> _$PutWritingStyleRequestToJson(
  PutWritingStyleRequest instance,
) => <String, dynamic>{
  'emoji': _$EmojiUseDtoEnumMap[instance.emoji],
  'hashtags': _$HashtagUseDtoEnumMap[instance.hashtags],
  'language': instance.language,
  'notes': instance.notes,
  'tone': instance.tone,
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
