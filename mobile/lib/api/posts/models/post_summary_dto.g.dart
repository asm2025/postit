// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostSummaryDto _$PostSummaryDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'created_at',
      'disclose_ai_generated',
      'id',
      'kind',
      'status',
      'tags',
      'text',
      'updated_at',
      'version',
    ],
    disallowNullValues: const [
      'created_at',
      'disclose_ai_generated',
      'id',
      'kind',
      'status',
      'tags',
      'text',
      'updated_at',
      'version',
    ],
  );
  return PostSummaryDto(
    campaignId: json['campaign_id'] as String?,
    createdAt: PostSummaryDto._read_createdAt(json['created_at']),
    discloseAiGenerated: json['disclose_ai_generated'] as bool,
    id: json['id'] as String,
    kind: PostSummaryDto._read_kind(json['kind']),
    link: json['link'] as String?,
    replyTo: json['reply_to'] == null
        ? null
        : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
    status: PostSummaryDto._read_status(json['status']),
    tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
    text: json['text'] as String,
    updatedAt: PostSummaryDto._read_updatedAt(json['updated_at']),
    version: PostSummaryDto._read_version(json['version']),
    additionalProperties:
        PostSummaryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PostSummaryDto._readExtras(
            PostSummaryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PostSummaryDtoToJson(PostSummaryDto instance) =>
    <String, dynamic>{
      'campaign_id': ?instance.campaignId,
      'created_at': ?PostSummaryDto._write_createdAt(instance.createdAt),
      'disclose_ai_generated': instance.discloseAiGenerated,
      'id': instance.id,
      'kind': ?PostSummaryDto._write_kind(instance.kind),
      'link': ?instance.link,
      'reply_to': ?instance.replyTo?.toJson(),
      'status': ?PostSummaryDto._write_status(instance.status),
      'tags': instance.tags,
      'text': instance.text,
      'updated_at': ?PostSummaryDto._write_updatedAt(instance.updatedAt),
      'version': ?PostSummaryDto._write_version(instance.version),
    };
