// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'generation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GenerationDto _$GenerationDtoFromJson(Map<String, dynamic> json) =>
    GenerationDto(
      draft: AiDraftDto.fromJson(json['draft'] as Map<String, dynamic>),
      generationId: json['generation_id'] as String,
      model: json['model'] as String,
      notes: (json['notes'] as List<dynamic>)
          .map((e) => Message.fromJson(e as Map<String, dynamic>))
          .toList(),
      providerId: json['provider_id'] as String,
      violations: (json['violations'] as Map<String, dynamic>).map(
        (k, e) => MapEntry(
          k,
          (e as List<dynamic>)
              .map((e) => Violation.fromJson(e as Map<String, dynamic>))
              .toList(),
        ),
      ),
      usage: json['usage'] == null
          ? null
          : AiUsageDto.fromJson(json['usage'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$GenerationDtoToJson(GenerationDto instance) =>
    <String, dynamic>{
      'draft': instance.draft,
      'generation_id': instance.generationId,
      'model': instance.model,
      'notes': instance.notes,
      'provider_id': instance.providerId,
      'usage': instance.usage,
      'violations': instance.violations,
    };
