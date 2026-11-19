// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostDto _$PostDtoFromJson(Map<String, dynamic> json) {
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
      'last_published',
      'media',
      'overrides',
      'targets',
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
      'last_published',
      'media',
      'overrides',
      'targets',
    ],
  );
  return PostDto(
    campaignId: json['campaign_id'] as String?,
    createdAt: PostDto._read_createdAt(json['created_at']),
    discloseAiGenerated: json['disclose_ai_generated'] as bool,
    id: json['id'] as String,
    kind: PostDto._read_kind(json['kind']),
    link: json['link'] as String?,
    replyTo: json['reply_to'] == null
        ? null
        : ReplyTo.fromJson(json['reply_to'] as Map<String, dynamic>),
    status: PostDto._read_status(json['status']),
    tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
    text: json['text'] as String,
    updatedAt: PostDto._read_updatedAt(json['updated_at']),
    version: PostDto._read_version(json['version']),
    lastPublished: (json['last_published'] as List<dynamic>)
        .map((e) => LastPublishedDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    media: (json['media'] as List<dynamic>)
        .map((e) => PostMediaDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    overrides: (json['overrides'] as List<dynamic>)
        .map((e) => PostOverrideDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    targets: (json['targets'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    additionalProperties: PostDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PostDto._readExtras(PostDto._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$PostDtoToJson(PostDto instance) => <String, dynamic>{
  'campaign_id': ?instance.campaignId,
  'created_at': ?PostDto._write_createdAt(instance.createdAt),
  'disclose_ai_generated': instance.discloseAiGenerated,
  'id': instance.id,
  'kind': ?PostDto._write_kind(instance.kind),
  'link': ?instance.link,
  'reply_to': ?instance.replyTo?.toJson(),
  'status': ?PostDto._write_status(instance.status),
  'tags': instance.tags,
  'text': instance.text,
  'updated_at': ?PostDto._write_updatedAt(instance.updatedAt),
  'version': ?PostDto._write_version(instance.version),
  'last_published': instance.lastPublished.map((e) => e.toJson()).toList(),
  'media': instance.media.map((e) => e.toJson()).toList(),
  'overrides': instance.overrides.map((e) => e.toJson()).toList(),
  'targets': instance.targets,
};
