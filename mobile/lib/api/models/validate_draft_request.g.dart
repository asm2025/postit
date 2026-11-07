// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validate_draft_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidateDraftRequest _$ValidateDraftRequestFromJson(
  Map<String, dynamic> json,
) => ValidateDraftRequest(
  at: json['at'] == null
      ? null
      : ScheduleInput.fromJson(json['at'] as Map<String, dynamic>),
  campaignId: json['campaign_id'] as String?,
  discloseAiGenerated: json['disclose_ai_generated'] as bool?,
  kind: json['kind'] == null ? null : PostKind.fromJson(json['kind'] as String),
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

Map<String, dynamic> _$ValidateDraftRequestToJson(
  ValidateDraftRequest instance,
) => <String, dynamic>{
  'at': instance.at,
  'campaign_id': instance.campaignId,
  'disclose_ai_generated': instance.discloseAiGenerated,
  'kind': _$PostKindEnumMap[instance.kind],
  'link': instance.link,
  'media': instance.media,
  'native_schedule': instance.nativeSchedule,
  'overrides': instance.overrides,
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
