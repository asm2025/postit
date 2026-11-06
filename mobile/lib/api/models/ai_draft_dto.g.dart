// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_draft_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiDraftDto _$AiDraftDtoFromJson(Map<String, dynamic> json) => AiDraftDto(
  overrides: (json['overrides'] as Map<String, dynamic>).map(
    (k, e) => MapEntry(k, PostOverride.fromJson(e as Map<String, dynamic>)),
  ),
  post: Post.fromJson(json['post'] as Map<String, dynamic>),
);

Map<String, dynamic> _$AiDraftDtoToJson(AiDraftDto instance) =>
    <String, dynamic>{'overrides': instance.overrides, 'post': instance.post};
