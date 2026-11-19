// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_post_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchPostRequest _$PatchPostRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'campaign_id',
      'disclose_ai_generated',
      'link',
      'media',
      'reply_to',
      'tags',
      'targets',
      'text',
    ],
  );
  return PatchPostRequest(
    campaignId: json['campaign_id'] as String?,
    discloseAiGenerated: json['disclose_ai_generated'] as bool?,
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

Map<String, dynamic> _$PatchPostRequestToJson(PatchPostRequest instance) =>
    <String, dynamic>{
      'campaign_id': ?instance.campaignId,
      'disclose_ai_generated': ?instance.discloseAiGenerated,
      'link': ?instance.link,
      'media': ?instance.media?.map((e) => e.toJson()).toList(),
      'reply_to': ?instance.replyTo?.toJson(),
      'tags': ?instance.tags,
      'targets': ?instance.targets,
      'text': ?instance.text,
    };
