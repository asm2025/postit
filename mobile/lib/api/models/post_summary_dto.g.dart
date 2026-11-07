// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostSummaryDto _$PostSummaryDtoFromJson(Map<String, dynamic> json) =>
    PostSummaryDto(
      createdAt: DateTime.parse(json['created_at'] as String),
      discloseAiGenerated: json['disclose_ai_generated'] as bool,
      id: json['id'] as String,
      kind: PostKind.fromJson(json['kind'] as String),
      status: PostStatusDto.fromJson(json['status'] as String),
      tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
      text: json['text'] as String,
      updatedAt: DateTime.parse(json['updated_at'] as String),
      version: (json['version'] as num).toInt(),
      campaignId: json['campaign_id'] as String?,
      link: json['link'] as String?,
      replyTo: json['reply_to'] == null
          ? null
          : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$PostSummaryDtoToJson(PostSummaryDto instance) =>
    <String, dynamic>{
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
