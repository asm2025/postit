// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'generation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GenerationDto _$GenerationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'draft',
      'generation_id',
      'model',
      'notes',
      'provider_id',
      'violations',
    ],
    disallowNullValues: const [
      'draft',
      'generation_id',
      'model',
      'notes',
      'provider_id',
      'violations',
    ],
  );
  return GenerationDto(
    draft: AiDraftDto.fromJson(json['draft'] as Map<String, dynamic>),
    generationId: json['generation_id'] as String,
    model: json['model'] as String,
    notes: (json['notes'] as List<dynamic>)
        .map((e) => Message.fromJson(e as Map<String, dynamic>))
        .toList(),
    providerId: json['provider_id'] as String,
    usage: json['usage'] == null
        ? null
        : AiUsageDto.fromJson(json['usage'] as Map<String, dynamic>),
    violations: (json['violations'] as Map<String, dynamic>).map(
      (k, e) => MapEntry(
        k,
        (e as List<dynamic>)
            .map((e) => Violation.fromJson(e as Map<String, dynamic>))
            .toList(),
      ),
    ),
    additionalProperties:
        GenerationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : GenerationDto._readExtras(
            GenerationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GenerationDtoToJson(GenerationDto instance) =>
    <String, dynamic>{
      'draft': instance.draft.toJson(),
      'generation_id': instance.generationId,
      'model': instance.model,
      'notes': instance.notes.map((e) => e.toJson()).toList(),
      'provider_id': instance.providerId,
      'usage': ?instance.usage?.toJson(),
      'violations': instance.violations.map(
        (k, e) => MapEntry(k, e.map((e) => e.toJson()).toList()),
      ),
    };
