// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'brand_profile_dto.g.dart';

/// What the owner says about their business, for the drafter. Reads as an empty profile with.
/// `id: null` until one is written.
@JsonSerializable()
class BrandProfileDto {
  const BrandProfileDto({
    required this.audience,
    required this.avoidWords,
    required this.description,
    required this.examples,
    required this.offerings,
    this.defaultLanguage,
    this.id,
    this.updatedAt,
  });
  
  factory BrandProfileDto.fromJson(Map<String, Object?> json) => _$BrandProfileDtoFromJson(json);
  
  /// Who it writes for.
  final String audience;

  /// Words the drafter must not use.
  @JsonKey(name: 'avoid_words')
  final List<String> avoidWords;

  /// The language to write in by default.
  @JsonKey(name: 'default_language')
  final String? defaultLanguage;

  /// What the business is.
  final String description;

  /// Posts that sound right, at most five.
  final List<String> examples;

  /// The profile, or `null` while none was written.
  final String? id;

  /// What it sells or offers.
  final String offerings;

  /// When it last changed, or `null` while none was written.
  @JsonKey(name: 'updated_at')
  final DateTime? updatedAt;

  Map<String, Object?> toJson() => _$BrandProfileDtoToJson(this);
}
