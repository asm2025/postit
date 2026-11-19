// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'change_delegation_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ChangeDelegationRequest _$ChangeDelegationRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['account_ids', 'all_accounts', 'expires_at', 'level'],
  );
  return ChangeDelegationRequest(
    accountIds: (json['account_ids'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    allAccounts: json['all_accounts'] as bool?,
    expiresAt: ChangeDelegationRequest._read_expiresAt(json['expires_at']),
    level: ChangeDelegationRequest._read_level(json['level']),
  );
}

Map<String, dynamic> _$ChangeDelegationRequestToJson(
  ChangeDelegationRequest instance,
) => <String, dynamic>{
  'account_ids': ?instance.accountIds,
  'all_accounts': ?instance.allAccounts,
  'expires_at': ?ChangeDelegationRequest._write_expiresAt(instance.expiresAt),
  'level': ?ChangeDelegationRequest._write_level(instance.level),
};
