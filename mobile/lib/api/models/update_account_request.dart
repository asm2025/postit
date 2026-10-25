// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'update_account_request.g.dart';

/// Settings the owner may change on an account.
@JsonSerializable()
class UpdateAccountRequest {
  const UpdateAccountRequest({
    this.displayName,
    this.testAccount,
  });
  
  factory UpdateAccountRequest.fromJson(Map<String, Object?> json) => _$UpdateAccountRequestFromJson(json);
  
  /// A new name, 1-100 characters.
  @JsonKey(name: 'display_name')
  final String? displayName;

  /// Mark or unmark the account as a test account.
  @JsonKey(name: 'test_account')
  final bool? testAccount;

  Map<String, Object?> toJson() => _$UpdateAccountRequestToJson(this);
}
