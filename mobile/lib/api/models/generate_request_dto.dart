// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_mode_dto.dart';

part 'generate_request_dto.g.dart';

/// A generation request. Nothing is published and no post is created.
@JsonSerializable()
class GenerateRequestDto {
  const GenerateRequestDto({
    required this.mode,
    required this.targets,
    this.commentId,
    this.language,
    this.link,
    this.mediaIds,
    this.model,
    this.prompt,
    this.providerId,
  });
  
  factory GenerateRequestDto.fromJson(Map<String, Object?> json) => _$GenerateRequestDtoFromJson(json);
  
  /// The inbox item to answer, in reply mode.
  @JsonKey(name: 'comment_id')
  final String? commentId;

  /// The language to write in, as a BCP 47 tag; else the account's writing style, the brand.
  /// profile, the comment's language, then the caller's interface language.
  final String? language;

  /// The link the post carries. The drafter never writes a link of its own.
  final String? link;

  /// Media the post carries, at most 10, in order; the model sees up to 4 images and 4.
  /// frames of each video when it reads images.
  @JsonKey(name: 'media_ids')
  final List<String>? mediaIds;

  /// What to write.
  final AiModeDto mode;

  /// The model; the provider's default when absent.
  final String? model;

  /// What to write about (at most 8,000 characters), or the text to rewrite. May be empty in.
  /// reply mode.
  final String? prompt;

  /// The provider; the owner's default when absent.
  @JsonKey(name: 'provider_id')
  final String? providerId;

  /// The accounts to write for, 1 to 20; exactly one, the comment's account, in reply mode.
  final List<String> targets;

  Map<String, Object?> toJson() => _$GenerateRequestDtoToJson(this);
}
