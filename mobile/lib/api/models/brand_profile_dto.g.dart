// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'brand_profile_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

BrandProfileDto _$BrandProfileDtoFromJson(Map<String, dynamic> json) =>
    BrandProfileDto(
      audience: json['audience'] as String,
      avoidWords: (json['avoid_words'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      description: json['description'] as String,
      examples: (json['examples'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      offerings: json['offerings'] as String,
      defaultLanguage: json['default_language'] as String?,
      id: json['id'] as String?,
      updatedAt: json['updated_at'] == null
          ? null
          : DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$BrandProfileDtoToJson(BrandProfileDto instance) =>
    <String, dynamic>{
      'audience': instance.audience,
      'avoid_words': instance.avoidWords,
      'default_language': instance.defaultLanguage,
      'description': instance.description,
      'examples': instance.examples,
      'id': instance.id,
      'offerings': instance.offerings,
      'updated_at': instance.updatedAt?.toIso8601String(),
    };
