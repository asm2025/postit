// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'generate_request_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GenerateRequestDto _$GenerateRequestDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'comment_id',
      'language',
      'link',
      'media_ids',
      'mode',
      'model',
      'prompt',
      'provider_id',
      'targets',
    ],
    requiredKeys: const ['mode', 'targets'],
    disallowNullValues: const ['media_ids', 'mode', 'prompt', 'targets'],
  );
  return GenerateRequestDto(
    commentId: json['comment_id'] as String?,
    language: json['language'] as String?,
    link: json['link'] as String?,
    mediaIds: (json['media_ids'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    mode: GenerateRequestDto._read_mode(json['mode']),
    model: json['model'] as String?,
    prompt: json['prompt'] as String?,
    providerId: json['provider_id'] as String?,
    targets: (json['targets'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
  );
}

Map<String, dynamic> _$GenerateRequestDtoToJson(GenerateRequestDto instance) =>
    <String, dynamic>{
      'comment_id': ?instance.commentId,
      'language': ?instance.language,
      'link': ?instance.link,
      'media_ids': ?instance.mediaIds,
      'mode': ?GenerateRequestDto._write_mode(instance.mode),
      'model': ?instance.model,
      'prompt': ?instance.prompt,
      'provider_id': ?instance.providerId,
      'targets': instance.targets,
    };
