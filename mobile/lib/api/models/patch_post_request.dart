// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_media_dto.dart';
import 'reply_to.dart';

part 'patch_post_request.g.dart';

/// Changes to a post. Absent fields stay as they are; `null` clears the reply target, the.
/// campaign, or the link. The kind never changes.
@JsonSerializable()
class PatchPostRequest {
  const PatchPostRequest({
    this.campaignId,
    this.discloseAiGenerated,
    this.link,
    this.media,
    this.replyTo,
    this.tags,
    this.targets,
    this.text,
  });
  
  factory PatchPostRequest.fromJson(Map<String, Object?> json) => _$PatchPostRequestFromJson(json);
  
  /// The campaign, or `null` to remove the post from it.
  @JsonKey(name: 'campaign_id')
  final String? campaignId;

  /// Whether the post says it was made with AI.
  @JsonKey(name: 'disclose_ai_generated')
  final bool? discloseAiGenerated;

  /// The link, or `null` to remove it.
  final String? link;

  /// The attachments, replacing the earlier ones.
  final List<PostMediaDto>? media;

  /// What a comment answers, or `null` to clear it.
  @JsonKey(name: 'reply_to')
  final ReplyTo? replyTo;

  /// The tags.
  final List<String>? tags;

  /// The targets, replacing the ones the caller can see; targets outside a limited.
  /// delegate's account limit stay.
  final List<String>? targets;

  /// The text.
  final String? text;

  Map<String, Object?> toJson() => _$PatchPostRequestToJson(this);
}
