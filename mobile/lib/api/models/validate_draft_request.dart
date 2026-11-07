// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_kind.dart';
import 'post_media_dto.dart';
import 'put_override_request.dart';
import 'reply_to.dart';
import 'schedule_input.dart';

part 'validate_draft_request.g.dart';

/// An unsaved post to validate: the `POST /posts` body, per-account overrides, and when it.
/// would go out.
@JsonSerializable()
class ValidateDraftRequest {
  const ValidateDraftRequest({
    this.at,
    this.campaignId,
    this.discloseAiGenerated,
    this.kind,
    this.link,
    this.media,
    this.nativeSchedule,
    this.overrides,
    this.replyTo,
    this.tags,
    this.targets,
    this.text,
  });
  
  factory ValidateDraftRequest.fromJson(Map<String, Object?> json) => _$ValidateDraftRequestFromJson(json);
  
  /// When it would go out; now when absent.
  final ScheduleInput? at;

  /// The campaign it belongs to.
  @JsonKey(name: 'campaign_id')
  final String? campaignId;

  /// Whether the post says it was made with AI.
  @JsonKey(name: 'disclose_ai_generated')
  final bool? discloseAiGenerated;

  /// Whether it starts a conversation or answers one (default `post`).
  final PostKind? kind;

  /// The link to attach, an absolute URL.
  final String? link;

  /// The attachments, in order. An asset that is not the owner's is a `media_missing`.
  /// violation, not a 404.
  final List<PostMediaDto>? media;

  /// Whether platforms that can hold the post until `at` would do so.
  @JsonKey(name: 'native_schedule')
  final bool? nativeSchedule;

  /// Per-account overrides, keyed by account ID.
  final Map<String, PutOverrideRequest>? overrides;

  /// What a comment answers; required for a comment.
  @JsonKey(name: 'reply_to')
  final ReplyTo? replyTo;

  /// The tags, without the leading `#`.
  final List<String>? tags;

  /// The accounts it would go to.
  final List<String>? targets;

  /// The text.
  final String? text;

  Map<String, Object?> toJson() => _$ValidateDraftRequestToJson(this);
}
