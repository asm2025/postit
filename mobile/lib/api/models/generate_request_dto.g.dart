// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'generate_request_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GenerateRequestDto _$GenerateRequestDtoFromJson(Map<String, dynamic> json) =>
    GenerateRequestDto(
      mode: AiModeDto.fromJson(json['mode'] as String),
      targets: (json['targets'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      commentId: json['comment_id'] as String?,
      language: json['language'] as String?,
      link: json['link'] as String?,
      mediaIds: (json['media_ids'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
      model: json['model'] as String?,
      prompt: json['prompt'] as String?,
      providerId: json['provider_id'] as String?,
    );

Map<String, dynamic> _$GenerateRequestDtoToJson(GenerateRequestDto instance) =>
    <String, dynamic>{
      'comment_id': instance.commentId,
      'language': instance.language,
      'link': instance.link,
      'media_ids': instance.mediaIds,
      'mode': _$AiModeDtoEnumMap[instance.mode]!,
      'model': instance.model,
      'prompt': instance.prompt,
      'provider_id': instance.providerId,
      'targets': instance.targets,
    };

const _$AiModeDtoEnumMap = {
  AiModeDto.post: 'post',
  AiModeDto.reply: 'reply',
  AiModeDto.rewrite: 'rewrite',
  AiModeDto.$unknown: r'$unknown',
};
