// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_draft_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiDraftDto _$AiDraftDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['overrides', 'post'],
    disallowNullValues: const ['overrides', 'post'],
  );
  return AiDraftDto(
    overrides: (json['overrides'] as Map<String, dynamic>).map(
      (k, e) => MapEntry(k, PostOverride.fromJson(e as Map<String, dynamic>)),
    ),
    post: Post.fromJson(json['post'] as Map<String, dynamic>),
    additionalProperties:
        AiDraftDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiDraftDto._readExtras(
            AiDraftDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiDraftDtoToJson(AiDraftDto instance) =>
    <String, dynamic>{
      'overrides': instance.overrides.map((k, e) => MapEntry(k, e.toJson())),
      'post': instance.post.toJson(),
    };
