// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'grant_delegation_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GrantDelegationRequest _$GrantDelegationRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['account_ids', 'delegate_id', 'expires_at', 'level'],
    requiredKeys: const ['delegate_id', 'level'],
    disallowNullValues: const ['delegate_id', 'level'],
  );
  return GrantDelegationRequest(
    accountIds: (json['account_ids'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    delegateId: json['delegate_id'] as String,
    expiresAt: GrantDelegationRequest._read_expiresAt(json['expires_at']),
    level: GrantDelegationRequest._read_level(json['level']),
  );
}

Map<String, dynamic> _$GrantDelegationRequestToJson(
  GrantDelegationRequest instance,
) => <String, dynamic>{
  'account_ids': ?instance.accountIds,
  'delegate_id': instance.delegateId,
  'expires_at': ?GrantDelegationRequest._write_expiresAt(instance.expiresAt),
  'level': ?GrantDelegationRequest._write_level(instance.level),
};
