// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_post_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreatePostRequest _$CreatePostRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'campaign_id',
      'disclose_ai_generated',
      'generation_id',
      'kind',
      'link',
      'media',
      'reply_to',
      'tags',
      'targets',
      'text',
    ],
    disallowNullValues: const [
      'disclose_ai_generated',
      'kind',
      'media',
      'tags',
      'targets',
      'text',
    ],
  );
  return CreatePostRequest(
    campaignId: json['campaign_id'] as String?,
    discloseAiGenerated: json['disclose_ai_generated'] as bool?,
    generationId: json['generation_id'] as String?,
    kind: CreatePostRequest._read_kind(json['kind']),
    link: json['link'] as String?,
    media: (json['media'] as List<dynamic>?)
        ?.map((e) => PostMediaDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    replyTo: json['reply_to'] == null
        ? null
        : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
    tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
    targets: (json['targets'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    text: json['text'] as String?,
  );
}

Map<String, dynamic> _$CreatePostRequestToJson(CreatePostRequest instance) =>
    <String, dynamic>{
      'campaign_id': ?instance.campaignId,
      'disclose_ai_generated': ?instance.discloseAiGenerated,
      'generation_id': ?instance.generationId,
      'kind': ?CreatePostRequest._write_kind(instance.kind),
      'link': ?instance.link,
      'media': ?instance.media?.map((e) => e.toJson()).toList(),
      'reply_to': ?instance.replyTo?.toJson(),
      'tags': ?instance.tags,
      'targets': ?instance.targets,
      'text': ?instance.text,
    };
