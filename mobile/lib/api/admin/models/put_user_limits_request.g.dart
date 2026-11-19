// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_user_limits_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutUserLimitsRequest _$PutUserLimitsRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'contacts_cap',
      'email_daily_cap',
      'storage_quota_bytes',
    ],
    requiredKeys: const [
      'contacts_cap',
      'email_daily_cap',
      'storage_quota_bytes',
    ],
    disallowNullValues: const [
      'contacts_cap',
      'email_daily_cap',
      'storage_quota_bytes',
    ],
  );
  return PutUserLimitsRequest(
    contactsCap: PutUserLimitsRequest._read_contactsCap(json['contacts_cap']),
    emailDailyCap: PutUserLimitsRequest._read_emailDailyCap(
      json['email_daily_cap'],
    ),
    storageQuotaBytes: PutUserLimitsRequest._read_storageQuotaBytes(
      json['storage_quota_bytes'],
    ),
  );
}

Map<String, dynamic> _$PutUserLimitsRequestToJson(
  PutUserLimitsRequest instance,
) => <String, dynamic>{
  'contacts_cap': ?PutUserLimitsRequest._write_contactsCap(
    instance.contactsCap,
  ),
  'email_daily_cap': ?PutUserLimitsRequest._write_emailDailyCap(
    instance.emailDailyCap,
  ),
  'storage_quota_bytes': ?PutUserLimitsRequest._write_storageQuotaBytes(
    instance.storageQuotaBytes,
  ),
};
