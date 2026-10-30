// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'account_label.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AccountLabel _$AccountLabelFromJson(Map<String, dynamic> json) => AccountLabel(
  accountId: json['account_id'] as String,
  accountName: json['account_name'] as String,
  platform: json['platform'] as String,
);

Map<String, dynamic> _$AccountLabelToJson(AccountLabel instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'account_name': instance.accountName,
      'platform': instance.platform,
    };
