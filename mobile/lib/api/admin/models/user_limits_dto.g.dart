// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_limits_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserLimitsDto _$UserLimitsDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'contacts_cap',
      'email_daily_cap',
      'storage_quota_bytes',
      'updated_at',
    ],
    disallowNullValues: const [
      'contacts_cap',
      'email_daily_cap',
      'storage_quota_bytes',
      'updated_at',
    ],
  );
  return UserLimitsDto(
    contactsCap: UserLimitsDto._read_contactsCap(json['contacts_cap']),
    emailDailyCap: UserLimitsDto._read_emailDailyCap(json['email_daily_cap']),
    storageQuotaBytes: UserLimitsDto._read_storageQuotaBytes(
      json['storage_quota_bytes'],
    ),
    updatedAt: UserLimitsDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        UserLimitsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : UserLimitsDto._readExtras(
            UserLimitsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$UserLimitsDtoToJson(UserLimitsDto instance) =>
    <String, dynamic>{
      'contacts_cap': ?UserLimitsDto._write_contactsCap(instance.contactsCap),
      'email_daily_cap': ?UserLimitsDto._write_emailDailyCap(
        instance.emailDailyCap,
      ),
      'storage_quota_bytes': ?UserLimitsDto._write_storageQuotaBytes(
        instance.storageQuotaBytes,
      ),
      'updated_at': ?UserLimitsDto._write_updatedAt(instance.updatedAt),
    };
