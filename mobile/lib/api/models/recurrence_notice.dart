// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_id.dart';
import 'recurrence_id.dart';
import 'skip_reason.dart';

part 'recurrence_notice.g.dart';

/// Payload of `recurrence_skipped`.
@JsonSerializable()
class RecurrenceNotice {
  const RecurrenceNotice({
    required this.postId,
    required this.reason,
    required this.recurrenceId,
  });
  
  factory RecurrenceNotice.fromJson(Map<String, Object?> json) => _$RecurrenceNoticeFromJson(json);
  
  /// Its post.
  @JsonKey(name: 'post_id')
  final PostId postId;

  /// Why it was skipped.
  final SkipReason reason;

  /// The recurrence.
  @JsonKey(name: 'recurrence_id')
  final RecurrenceId recurrenceId;

  Map<String, Object?> toJson() => _$RecurrenceNoticeToJson(this);
}
