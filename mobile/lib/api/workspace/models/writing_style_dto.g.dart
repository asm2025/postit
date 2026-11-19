// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'writing_style_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

WritingStyleDto _$WritingStyleDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_id', 'emoji', 'hashtags', 'notes', 'tone'],
    disallowNullValues: const [
      'account_id',
      'emoji',
      'hashtags',
      'notes',
      'tone',
    ],
  );
  return WritingStyleDto(
    accountId: json['account_id'] as String,
    emoji: WritingStyleDto._read_emoji(json['emoji']),
    hashtags: WritingStyleDto._read_hashtags(json['hashtags']),
    id: json['id'] as String?,
    language: json['language'] as String?,
    notes: json['notes'] as String,
    tone: json['tone'] as String,
    updatedAt: WritingStyleDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        WritingStyleDto._extras(json, 'additionalProperties') == null
        ? const {}
        : WritingStyleDto._readExtras(
            WritingStyleDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$WritingStyleDtoToJson(WritingStyleDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'emoji': ?WritingStyleDto._write_emoji(instance.emoji),
      'hashtags': ?WritingStyleDto._write_hashtags(instance.hashtags),
      'id': ?instance.id,
      'language': ?instance.language,
      'notes': instance.notes,
      'tone': instance.tone,
      'updated_at': ?WritingStyleDto._write_updatedAt(instance.updatedAt),
    };
