// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_user_limits_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutUserLimitsRequest _$PutUserLimitsRequestFromJson(
  Map<String, dynamic> json,
) => PutUserLimitsRequest(
  contactsCap: (json['contacts_cap'] as num).toInt(),
  emailDailyCap: (json['email_daily_cap'] as num).toInt(),
  storageQuotaBytes: (json['storage_quota_bytes'] as num).toInt(),
);

Map<String, dynamic> _$PutUserLimitsRequestToJson(
  PutUserLimitsRequest instance,
) => <String, dynamic>{
  'contacts_cap': instance.contactsCap,
  'email_daily_cap': instance.emailDailyCap,
  'storage_quota_bytes': instance.storageQuotaBytes,
};
