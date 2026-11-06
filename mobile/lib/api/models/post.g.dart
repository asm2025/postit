// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Post _$PostFromJson(Map<String, dynamic> json) => Post(
  kind: PostKind.fromJson(json['kind'] as String),
  text: json['text'] as String,
  discloseAiGenerated: json['disclose_ai_generated'] as bool?,
  link: json['link'] as String?,
  media: (json['media'] as List<dynamic>?)
      ?.map((e) => MediaItem.fromJson(e as Map<String, dynamic>))
      .toList(),
  replyTo: json['reply_to'] == null
      ? null
      : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
  tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
);

Map<String, dynamic> _$PostToJson(Post instance) => <String, dynamic>{
  'disclose_ai_generated': instance.discloseAiGenerated,
  'kind': _$PostKindEnumMap[instance.kind]!,
  'link': instance.link,
  'media': instance.media,
  'reply_to': instance.replyTo,
  'tags': instance.tags,
  'text': instance.text,
};

const _$PostKindEnumMap = {
  PostKind.post: 'post',
  PostKind.comment: 'comment',
  PostKind.$unknown: r'$unknown',
};
