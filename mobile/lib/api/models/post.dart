// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'media_item.dart';
import 'post_kind.dart';
import 'reply_to.dart';

part 'post.g.dart';

/// The content shared by every target of a publish.
@JsonSerializable()
class Post {
  const Post({
    required this.kind,
    required this.text,
    this.discloseAiGenerated,
    this.link,
    this.media,
    this.replyTo,
    this.tags,
  });
  
  factory Post.fromJson(Map<String, Object?> json) => _$PostFromJson(json);
  
  /// Whether the post says it was made with AI.
  @JsonKey(name: 'disclose_ai_generated')
  final bool? discloseAiGenerated;

  /// Standalone post or reply.
  final PostKind kind;

  /// A link to attach.
  final String? link;

  /// Attached media, in display order.
  final List<MediaItem>? media;

  /// What a comment answers; required for [`PostKind::Comment`].
  @JsonKey(name: 'reply_to')
  final ReplyTo? replyTo;

  /// Hashtags, without the leading `#`.
  final List<String>? tags;

  /// The text.
  final String text;

  Map<String, Object?> toJson() => _$PostToJson(this);
}
