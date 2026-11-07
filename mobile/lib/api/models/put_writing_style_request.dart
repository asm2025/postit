// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'emoji_use_dto.dart';
import 'hashtag_use_dto.dart';

part 'put_writing_style_request.g.dart';

/// The writing style to store; it replaces the earlier one. Absent fields are empty.
@JsonSerializable()
class PutWritingStyleRequest {
  const PutWritingStyleRequest({
    this.emoji,
    this.hashtags,
    this.language,
    this.notes,
    this.tone,
  });
  
  factory PutWritingStyleRequest.fromJson(Map<String, Object?> json) => _$PutWritingStyleRequestFromJson(json);
  
  /// How much emoji to use.
  final EmojiUseDto? emoji;

  /// How many hashtags to use.
  final HashtagUseDto? hashtags;

  /// The language to write in, when it differs from the brand's default.
  final String? language;

  /// Anything else the drafter should know.
  final String? notes;

  /// The tone, in the owner's words.
  final String? tone;

  Map<String, Object?> toJson() => _$PutWritingStyleRequestToJson(this);
}
