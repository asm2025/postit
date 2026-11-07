// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_post_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreatePostRequest _$CreatePostRequestFromJson(Map<String, dynamic> json) =>
    CreatePostRequest(
      campaignId: json['campaign_id'] as String?,
      discloseAiGenerated: json['disclose_ai_generated'] as bool?,
      generationId: json['generation_id'] as String?,
      kind: json['kind'] == null
          ? null
          : PostKind.fromJson(json['kind'] as String),
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

Map<String, dynamic> _$CreatePostRequestToJson(CreatePostRequest instance) =>
    <String, dynamic>{
      'campaign_id': instance.campaignId,
      'disclose_ai_generated': instance.discloseAiGenerated,
      'generation_id': instance.generationId,
      'kind': _$PostKindEnumMap[instance.kind],
      'link': instance.link,
      'media': instance.media,
      'reply_to': instance.replyTo,
      'tags': instance.tags,
      'targets': instance.targets,
      'text': instance.text,
    };

const _$PostKindEnumMap = {
  PostKind.post: 'post',
  PostKind.comment: 'comment',
  PostKind.$unknown: r'$unknown',
};
