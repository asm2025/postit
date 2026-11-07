// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'user_limits_dto.g.dart';

/// A user's limits. `0` means unlimited.
@JsonSerializable()
class UserLimitsDto {
  const UserLimitsDto({
    required this.contactsCap,
    required this.emailDailyCap,
    required this.storageQuotaBytes,
    required this.updatedAt,
  });
  
  factory UserLimitsDto.fromJson(Map<String, Object?> json) => _$UserLimitsDtoFromJson(json);
  
  /// Contacts the user may keep; `0` is unlimited.
  @JsonKey(name: 'contacts_cap')
  final int contactsCap;

  /// Emails the user may send per day; `0` is unlimited.
  @JsonKey(name: 'email_daily_cap')
  final int emailDailyCap;

  /// Bytes of media the user may store; `0` is unlimited.
  @JsonKey(name: 'storage_quota_bytes')
  final int storageQuotaBytes;

  /// When the limits last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$UserLimitsDtoToJson(this);
}
