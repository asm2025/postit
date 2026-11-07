// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_brand_profile_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutBrandProfileRequest _$PutBrandProfileRequestFromJson(
  Map<String, dynamic> json,
) => PutBrandProfileRequest(
  audience: json['audience'] as String?,
  avoidWords: (json['avoid_words'] as List<dynamic>?)
      ?.map((e) => e as String)
      .toList(),
  defaultLanguage: json['default_language'] as String?,
  description: json['description'] as String?,
  examples: (json['examples'] as List<dynamic>?)
      ?.map((e) => e as String)
      .toList(),
  offerings: json['offerings'] as String?,
);

Map<String, dynamic> _$PutBrandProfileRequestToJson(
  PutBrandProfileRequest instance,
) => <String, dynamic>{
  'audience': instance.audience,
  'avoid_words': instance.avoidWords,
  'default_language': instance.defaultLanguage,
  'description': instance.description,
  'examples': instance.examples,
  'offerings': instance.offerings,
};
