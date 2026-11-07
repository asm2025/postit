// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostDto _$PostDtoFromJson(Map<String, dynamic> json) => PostDto(
  createdAt: DateTime.parse(json['created_at'] as String),
  discloseAiGenerated: json['disclose_ai_generated'] as bool,
  id: json['id'] as String,
  kind: PostKind.fromJson(json['kind'] as String),
  status: PostStatusDto.fromJson(json['status'] as String),
  tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
  text: json['text'] as String,
  updatedAt: DateTime.parse(json['updated_at'] as String),
  version: (json['version'] as num).toInt(),
  lastPublished: (json['last_published'] as List<dynamic>)
      .map((e) => LastPublishedDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  media: (json['media'] as List<dynamic>)
      .map((e) => PostMediaDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  overrides: (json['overrides'] as List<dynamic>)
      .map((e) => PostOverrideDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  targets: (json['targets'] as List<dynamic>).map((e) => e as String).toList(),
  campaignId: json['campaign_id'] as String?,
  link: json['link'] as String?,
  replyTo: json['reply_to'] == null
      ? null
      : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
);

Map<String, dynamic> _$PostDtoToJson(PostDto instance) => <String, dynamic>{
  'campaign_id': instance.campaignId,
  'created_at': instance.createdAt.toIso8601String(),
  'disclose_ai_generated': instance.discloseAiGenerated,
  'id': instance.id,
  'kind': _$PostKindEnumMap[instance.kind]!,
  'link': instance.link,
  'reply_to': instance.replyTo,
  'status': _$PostStatusDtoEnumMap[instance.status]!,
  'tags': instance.tags,
  'text': instance.text,
  'updated_at': instance.updatedAt.toIso8601String(),
  'version': instance.version,
  'last_published': instance.lastPublished,
  'media': instance.media,
  'overrides': instance.overrides,
  'targets': instance.targets,
};

const _$PostKindEnumMap = {
  PostKind.post: 'post',
  PostKind.comment: 'comment',
  PostKind.$unknown: r'$unknown',
};

const _$PostStatusDtoEnumMap = {
  PostStatusDto.draft: 'draft',
  PostStatusDto.scheduled: 'scheduled',
  PostStatusDto.publishing: 'publishing',
  PostStatusDto.published: 'published',
  PostStatusDto.partial: 'partial',
  PostStatusDto.failed: 'failed',
  PostStatusDto.cancelled: 'cancelled',
  PostStatusDto.$unknown: r'$unknown',
};
