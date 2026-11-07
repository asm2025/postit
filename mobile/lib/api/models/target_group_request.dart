// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'target_group_request.g.dart';

/// A target group to create, or the whole group a `PUT` stores.
@JsonSerializable()
class TargetGroupRequest {
  const TargetGroupRequest({
    required this.name,
    this.accountIds,
  });
  
  factory TargetGroupRequest.fromJson(Map<String, Object?> json) => _$TargetGroupRequestFromJson(json);
  
  /// Its member accounts. A delegate with a limited account list keeps the members they.
  /// cannot see; naming one they cannot see is `account_not_in_scope`.
  @JsonKey(name: 'account_ids')
  final List<String>? accountIds;

  /// Its name.
  final String name;

  Map<String, Object?> toJson() => _$TargetGroupRequestToJson(this);
}
