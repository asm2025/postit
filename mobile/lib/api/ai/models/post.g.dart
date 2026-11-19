// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Post _$PostFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['kind', 'text'],
    disallowNullValues: const [
      'disclose_ai_generated',
      'kind',
      'media',
      'tags',
      'text',
    ],
  );
  return Post(
    discloseAiGenerated: json['disclose_ai_generated'] as bool?,
    kind: Post._read_kind(json['kind']),
    link: json['link'] as String?,
    media: (json['media'] as List<dynamic>?)
        ?.map((e) => MediaItem.fromJson(e as Map<String, dynamic>))
        .toList(),
    replyTo: json['reply_to'] == null
        ? null
        : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
    tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
    text: json['text'] as String,
    additionalProperties: Post._extras(json, 'additionalProperties') == null
        ? const {}
        : Post._readExtras(Post._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$PostToJson(Post instance) => <String, dynamic>{
  'disclose_ai_generated': ?instance.discloseAiGenerated,
  'kind': ?Post._write_kind(instance.kind),
  'link': ?instance.link,
  'media': ?instance.media?.map((e) => e.toJson()).toList(),
  'reply_to': ?instance.replyTo?.toJson(),
  'tags': ?instance.tags,
  'text': instance.text,
};
