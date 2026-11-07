// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'emoji_use_dto.dart';
import 'hashtag_use_dto.dart';

part 'writing_style_dto.g.dart';

/// How the drafter writes for one account. Reads as an empty style with `id: null` until one.
/// is written.
@JsonSerializable()
class WritingStyleDto {
  const WritingStyleDto({
    required this.accountId,
    required this.emoji,
    required this.hashtags,
    required this.notes,
    required this.tone,
    this.id,
    this.language,
    this.updatedAt,
  });
  
  factory WritingStyleDto.fromJson(Map<String, Object?> json) => _$WritingStyleDtoFromJson(json);
  
  /// The account it guides.
  @JsonKey(name: 'account_id')
  final String accountId;

  /// How much emoji to use.
  final EmojiUseDto emoji;

  /// How many hashtags to use.
  final HashtagUseDto hashtags;

  /// The style, or `null` while none was written.
  final String? id;

  /// The language to write in, when it differs from the brand's default.
  final String? language;

  /// Anything else the drafter should know.
  final String notes;

  /// The tone, in the owner's words.
  final String tone;

  /// When it last changed, or `null` while none was written.
  @JsonKey(name: 'updated_at')
  final DateTime? updatedAt;

  Map<String, Object?> toJson() => _$WritingStyleDtoToJson(this);
}
