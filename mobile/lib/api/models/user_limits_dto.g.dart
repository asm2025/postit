// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_limits_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserLimitsDto _$UserLimitsDtoFromJson(Map<String, dynamic> json) =>
    UserLimitsDto(
      contactsCap: (json['contacts_cap'] as num).toInt(),
      emailDailyCap: (json['email_daily_cap'] as num).toInt(),
      storageQuotaBytes: (json['storage_quota_bytes'] as num).toInt(),
      updatedAt: DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$UserLimitsDtoToJson(UserLimitsDto instance) =>
    <String, dynamic>{
      'contacts_cap': instance.contactsCap,
      'email_daily_cap': instance.emailDailyCap,
      'storage_quota_bytes': instance.storageQuotaBytes,
      'updated_at': instance.updatedAt.toIso8601String(),
    };
