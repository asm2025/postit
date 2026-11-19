// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_writing_style_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutWritingStyleRequest _$PutWritingStyleRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['emoji', 'hashtags', 'language', 'notes', 'tone'],
    disallowNullValues: const ['emoji', 'hashtags', 'notes', 'tone'],
  );
  return PutWritingStyleRequest(
    emoji: PutWritingStyleRequest._read_emoji(json['emoji']),
    hashtags: PutWritingStyleRequest._read_hashtags(json['hashtags']),
    language: json['language'] as String?,
    notes: json['notes'] as String?,
    tone: json['tone'] as String?,
  );
}

Map<String, dynamic> _$PutWritingStyleRequestToJson(
  PutWritingStyleRequest instance,
) => <String, dynamic>{
  'emoji': ?PutWritingStyleRequest._write_emoji(instance.emoji),
  'hashtags': ?PutWritingStyleRequest._write_hashtags(instance.hashtags),
  'language': ?instance.language,
  'notes': ?instance.notes,
  'tone': ?instance.tone,
};
