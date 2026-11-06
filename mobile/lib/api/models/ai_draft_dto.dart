// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post.dart';
import 'post_override.dart';

part 'ai_draft_dto.g.dart';

/// The draft, in the shape of a post plus per-account overrides, ready for the post editor.
@JsonSerializable()
class AiDraftDto {
  const AiDraftDto({
    required this.overrides,
    required this.post,
  });
  
  factory AiDraftDto.fromJson(Map<String, Object?> json) => _$AiDraftDtoFromJson(json);
  
  /// One override per requested target, by account ID.
  final Map<String, PostOverride> overrides;

  /// The shared post: the first target's text and tags, the media, the link, and.
  /// `disclose_ai_generated: true`.
  final Post post;

  Map<String, Object?> toJson() => _$AiDraftDtoToJson(this);
}
