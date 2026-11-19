// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validate_draft_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidateDraftRequest _$ValidateDraftRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'at',
      'campaign_id',
      'disclose_ai_generated',
      'kind',
      'link',
      'media',
      'native_schedule',
      'overrides',
      'reply_to',
      'tags',
      'targets',
      'text',
    ],
    disallowNullValues: const [
      'disclose_ai_generated',
      'kind',
      'media',
      'native_schedule',
      'overrides',
      'tags',
      'targets',
      'text',
    ],
  );
  return ValidateDraftRequest(
    at: !json.containsKey('at')
        ? null
        : ValidateDraftRequest._read_at(json['at']),
    campaignId: json['campaign_id'] as String?,
    discloseAiGenerated: json['disclose_ai_generated'] as bool?,
    kind: ValidateDraftRequest._read_kind(json['kind']),
    link: json['link'] as String?,
    media: (json['media'] as List<dynamic>?)
        ?.map((e) => PostMediaDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    nativeSchedule: json['native_schedule'] as bool?,
    overrides: (json['overrides'] as Map<String, dynamic>?)?.map(
      (k, e) =>
          MapEntry(k, PutOverrideRequest.fromJson(e as Map<String, dynamic>)),
    ),
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

Map<String, dynamic> _$ValidateDraftRequestToJson(
  ValidateDraftRequest instance,
) => <String, dynamic>{
  if (instance.at case final $value?)
    'at': ValidateDraftRequest._write_at($value),
  'campaign_id': ?instance.campaignId,
  'disclose_ai_generated': ?instance.discloseAiGenerated,
  'kind': ?ValidateDraftRequest._write_kind(instance.kind),
  'link': ?instance.link,
  'media': ?instance.media?.map((e) => e.toJson()).toList(),
  'native_schedule': ?instance.nativeSchedule,
  'overrides': ?instance.overrides?.map((k, e) => MapEntry(k, e.toJson())),
  'reply_to': ?instance.replyTo?.toJson(),
  'tags': ?instance.tags,
  'targets': ?instance.targets,
  'text': ?instance.text,
};
