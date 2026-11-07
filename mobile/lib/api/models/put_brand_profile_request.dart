// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'put_brand_profile_request.g.dart';

/// The brand profile to store; it replaces the earlier one. Absent fields are empty.
@JsonSerializable()
class PutBrandProfileRequest {
  const PutBrandProfileRequest({
    this.audience,
    this.avoidWords,
    this.defaultLanguage,
    this.description,
    this.examples,
    this.offerings,
  });
  
  factory PutBrandProfileRequest.fromJson(Map<String, Object?> json) => _$PutBrandProfileRequestFromJson(json);
  
  /// Who it writes for.
  final String? audience;

  /// Words the drafter must not use.
  @JsonKey(name: 'avoid_words')
  final List<String>? avoidWords;

  /// The language to write in by default.
  @JsonKey(name: 'default_language')
  final String? defaultLanguage;

  /// What the business is.
  final String? description;

  /// Posts that sound right, at most five (`validation_failed` with `field: "examples"`).
  final List<String>? examples;

  /// What it sells or offers.
  final String? offerings;

  Map<String, Object?> toJson() => _$PutBrandProfileRequestToJson(this);
}
