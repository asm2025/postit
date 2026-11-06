// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_override.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostOverride _$PostOverrideFromJson(Map<String, dynamic> json) => PostOverride(
  email: json['email'] == null
      ? null
      : EmailDocument.fromJson(json['email'] as Map<String, dynamic>),
  link: json['link'] as String?,
  media: (json['media'] as List<dynamic>?)?.map((e) => e as String).toList(),
  options: json['options'],
  tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
  text: json['text'] as String?,
);

Map<String, dynamic> _$PostOverrideToJson(PostOverride instance) =>
    <String, dynamic>{
      'email': instance.email,
      'link': instance.link,
      'media': instance.media,
      'options': instance.options,
      'tags': instance.tags,
      'text': instance.text,
    };
