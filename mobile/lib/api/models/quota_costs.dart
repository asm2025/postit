// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'quota_costs.g.dart';

/// What each operation costs against a bucket. 0 means the operation does not count.
@JsonSerializable()
class QuotaCosts {
  const QuotaCosts({
    required this.comment,
    required this.inboxPoll,
    required this.metrics,
    required this.publish,
  });
  
  factory QuotaCosts.fromJson(Map<String, Object?> json) => _$QuotaCostsFromJson(json);
  
  /// Cost of one comment.
  final int comment;

  /// Cost of one inbox poll.
  @JsonKey(name: 'inbox_poll')
  final int inboxPoll;

  /// Cost of one metrics read.
  final int metrics;

  /// Cost of one publish.
  final int publish;

  Map<String, Object?> toJson() => _$QuotaCostsToJson(this);
}
