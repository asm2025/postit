// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'reply_to.g.dart';

/// What a comment answers, in the platform's own terms.
@JsonSerializable()
class ReplyTo {
  const ReplyTo({
    required this.remotePostId,
    this.remoteCommentId,
    this.url,
  });
  
  factory ReplyTo.fromJson(Map<String, Object?> json) => _$ReplyToFromJson(json);
  
  /// The platform's ID of the comment being answered, when replying to a comment.
  @JsonKey(name: 'remote_comment_id')
  final String? remoteCommentId;

  /// The platform's ID of the post being answered.
  @JsonKey(name: 'remote_post_id')
  final String remotePostId;

  /// A link to what is being answered.
  final String? url;

  Map<String, Object?> toJson() => _$ReplyToToJson(this);
}
