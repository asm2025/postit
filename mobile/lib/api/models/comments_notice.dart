// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_label.dart';

part 'comments_notice.g.dart';

/// Payload of `new_comments`.
@JsonSerializable()
class CommentsNotice {
  const CommentsNotice({
    required this.account,
    required this.count,
  });
  
  factory CommentsNotice.fromJson(Map<String, Object?> json) => _$CommentsNoticeFromJson(json);
  
  /// The account the comments are on.
  final AccountLabel account;

  /// How many new comments.
  final int count;

  Map<String, Object?> toJson() => _$CommentsNoticeToJson(this);
}
