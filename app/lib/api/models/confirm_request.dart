// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'confirm_request.g.dart';

/// Which accounts to connect.
@JsonSerializable()
class ConfirmRequest {
  const ConfirmRequest({
    this.accountIds,
  });
  
  factory ConfirmRequest.fromJson(Map<String, Object?> json) => _$ConfirmRequestFromJson(json);
  
  /// The `remote_id` of each account to connect; omit to connect every discovered account.
  @JsonKey(name: 'account_ids')
  final List<String>? accountIds;

  Map<String, Object?> toJson() => _$ConfirmRequestToJson(this);
}
