// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_kind.dart';
import 'post_status_dto.dart';
import 'reply_to.dart';

part 'post_summary_dto.g.dart';

/// A post without its media, targets, or overrides: an item of the post list.
@JsonSerializable()
class PostSummaryDto {
  const PostSummaryDto({
    required this.createdAt,
    required this.discloseAiGenerated,
    required this.id,
    required this.kind,
    required this.status,
    required this.tags,
    required this.text,
    required this.updatedAt,
    required this.version,
    this.campaignId,
    this.link,
    this.replyTo,
  });
  
  factory PostSummaryDto.fromJson(Map<String, Object?> json) => _$PostSummaryDtoFromJson(json);
  
  /// The campaign it belongs to.
  @JsonKey(name: 'campaign_id')
  final String? campaignId;

  /// When it was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Whether the post says it was made with AI.
  @JsonKey(name: 'disclose_ai_generated')
  final bool discloseAiGenerated;

  /// The post.
  final String id;

  /// Whether it starts a conversation or answers one.
  final PostKind kind;

  /// The link to attach.
  final String? link;

  /// What a comment answers.
  @JsonKey(name: 'reply_to')
  final ReplyTo? replyTo;

  /// Where it is in its publishing life.
  final PostStatusDto status;

  /// The tags, without the leading `#`.
  final List<String> tags;

  /// The text.
  final String text;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  /// Counts content changes, starting at 1; the `ETag` is `"v{version}"`.
  final int version;

  Map<String, Object?> toJson() => _$PostSummaryDtoToJson(this);
}
