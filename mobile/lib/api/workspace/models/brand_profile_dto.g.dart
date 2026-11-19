// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'brand_profile_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

BrandProfileDto _$BrandProfileDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'audience',
      'avoid_words',
      'description',
      'examples',
      'offerings',
    ],
    disallowNullValues: const [
      'audience',
      'avoid_words',
      'description',
      'examples',
      'offerings',
    ],
  );
  return BrandProfileDto(
    audience: json['audience'] as String,
    avoidWords: (json['avoid_words'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    defaultLanguage: json['default_language'] as String?,
    description: json['description'] as String,
    examples: (json['examples'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    id: json['id'] as String?,
    offerings: json['offerings'] as String,
    updatedAt: BrandProfileDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        BrandProfileDto._extras(json, 'additionalProperties') == null
        ? const {}
        : BrandProfileDto._readExtras(
            BrandProfileDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$BrandProfileDtoToJson(BrandProfileDto instance) =>
    <String, dynamic>{
      'audience': instance.audience,
      'avoid_words': instance.avoidWords,
      'default_language': ?instance.defaultLanguage,
      'description': instance.description,
      'examples': instance.examples,
      'id': ?instance.id,
      'offerings': instance.offerings,
      'updated_at': ?BrandProfileDto._write_updatedAt(instance.updatedAt),
    };
