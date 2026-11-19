// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_override.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostOverride _$PostOverrideFromJson(Map<String, dynamic> json) {
  $checkKeys(json, disallowNullValues: const ['options']);
  return PostOverride(
    email: json['email'] == null
        ? null
        : EmailDocument.fromJson(json['email'] as Map<String, dynamic>),
    link: json['link'] as String?,
    media: PostOverride._read_media(json['media']),
    options: json['options'] == null
        ? null
        : PostOverrideOptions.fromJson(json['options'] as Map<String, dynamic>),
    tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
    text: json['text'] as String?,
    additionalProperties:
        PostOverride._extras(json, 'additionalProperties') == null
        ? const {}
        : PostOverride._readExtras(
            PostOverride._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PostOverrideToJson(PostOverride instance) =>
    <String, dynamic>{
      'email': ?instance.email?.toJson(),
      'link': ?instance.link,
      'media': ?PostOverride._write_media(instance.media),
      'options': ?instance.options?.toJson(),
      'tags': ?instance.tags,
      'text': ?instance.text,
    };
