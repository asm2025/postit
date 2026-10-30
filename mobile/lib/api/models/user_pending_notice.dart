// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'user_id.dart';

part 'user_pending_notice.g.dart';

/// Payload of `user_pending`.
@JsonSerializable()
class UserPendingNotice {
  const UserPendingNotice({
    required this.userId,
  });
  
  factory UserPendingNotice.fromJson(Map<String, Object?> json) => _$UserPendingNoticeFromJson(json);
  
  /// The user waiting for approval.
  @JsonKey(name: 'user_id')
  final UserId userId;

  Map<String, Object?> toJson() => _$UserPendingNoticeToJson(this);
}
