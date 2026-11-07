// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'put_user_limits_request.g.dart';

/// New limits for a user. `0` means unlimited; a negative number is `validation_failed`.
@JsonSerializable()
class PutUserLimitsRequest {
  const PutUserLimitsRequest({
    required this.contactsCap,
    required this.emailDailyCap,
    required this.storageQuotaBytes,
  });
  
  factory PutUserLimitsRequest.fromJson(Map<String, Object?> json) => _$PutUserLimitsRequestFromJson(json);
  
  /// Contacts the user may keep; `0` is unlimited.
  @JsonKey(name: 'contacts_cap')
  final int contactsCap;

  /// Emails the user may send per day; `0` is unlimited.
  @JsonKey(name: 'email_daily_cap')
  final int emailDailyCap;

  /// Bytes of media the user may store; `0` is unlimited.
  @JsonKey(name: 'storage_quota_bytes')
  final int storageQuotaBytes;

  Map<String, Object?> toJson() => _$PutUserLimitsRequestToJson(this);
}
