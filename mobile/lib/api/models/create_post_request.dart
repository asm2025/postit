// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_kind.dart';
import 'post_media_dto.dart';
import 'reply_to.dart';

part 'create_post_request.g.dart';

/// A post to create (or to validate without saving).
@JsonSerializable()
class CreatePostRequest {
  const CreatePostRequest({
    this.campaignId,
    this.discloseAiGenerated,
    this.generationId,
    this.kind,
    this.link,
    this.media,
    this.replyTo,
    this.tags,
    this.targets,
    this.text,
  });
  
  factory CreatePostRequest.fromJson(Map<String, Object?> json) => _$CreatePostRequestFromJson(json);
  
  /// The campaign it belongs to.
  @JsonKey(name: 'campaign_id')
  final String? campaignId;

  /// Whether the post says it was made with AI.
  @JsonKey(name: 'disclose_ai_generated')
  final bool? discloseAiGenerated;

  /// The AI generation this post was saved from; it links to the post.
  @JsonKey(name: 'generation_id')
  final String? generationId;

  /// Whether it starts a conversation or answers one (default `post`).
  final PostKind? kind;

  /// The link to attach, an absolute URL.
  final String? link;

  /// The attachments, in order (the position field is ignored).
  final List<PostMediaDto>? media;

  /// What a comment answers; required for a comment.
  @JsonKey(name: 'reply_to')
  final ReplyTo? replyTo;

  /// The tags, without the leading `#`.
  final List<String>? tags;

  /// The accounts it goes to.
  final List<String>? targets;

  /// The text.
  final String? text;

  Map<String, Object?> toJson() => _$CreatePostRequestToJson(this);
}
