// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'account_label.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AccountLabel _$AccountLabelFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_id', 'account_name', 'platform'],
    disallowNullValues: const ['account_name'],
  );
  return AccountLabel(
    accountId: AccountLabel._read_accountId(json['account_id']),
    accountName: json['account_name'] as String,
    platform: AccountLabel._read_platform(json['platform']),
    additionalProperties:
        AccountLabel._extras(json, 'additionalProperties') == null
        ? const {}
        : AccountLabel._readExtras(
            AccountLabel._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AccountLabelToJson(AccountLabel instance) =>
    <String, dynamic>{
      'account_id': AccountLabel._write_accountId(instance.accountId),
      'account_name': instance.accountName,
      'platform': AccountLabel._write_platform(instance.platform),
    };
