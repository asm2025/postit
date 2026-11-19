// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AccountDto _$AccountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'created_at',
      'display_name',
      'id',
      'platform',
      'remote_account_id',
      'status',
      'test_account',
      'updated_at',
    ],
    disallowNullValues: const [
      'created_at',
      'display_name',
      'id',
      'platform',
      'remote_account_id',
      'status',
      'test_account',
      'updated_at',
    ],
  );
  return AccountDto(
    avatarUrl: json['avatar_url'] as String?,
    connect: AccountDto._read_connect(json['connect']),
    createdAt: AccountDto._read_createdAt(json['created_at']),
    displayName: json['display_name'] as String,
    id: json['id'] as String,
    lastCheckedAt: AccountDto._read_lastCheckedAt(json['last_checked_at']),
    platform: json['platform'] as String,
    remoteAccountId: json['remote_account_id'] as String,
    status: AccountDto._read_status(json['status']),
    testAccount: json['test_account'] as bool,
    updatedAt: AccountDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        AccountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AccountDto._readExtras(
            AccountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AccountDtoToJson(
  AccountDto instance,
) => <String, dynamic>{
  'avatar_url': ?instance.avatarUrl,
  'connect': ?AccountDto._write_connect(instance.connect),
  'created_at': ?AccountDto._write_createdAt(instance.createdAt),
  'display_name': instance.displayName,
  'id': instance.id,
  'last_checked_at': ?AccountDto._write_lastCheckedAt(instance.lastCheckedAt),
  'platform': instance.platform,
  'remote_account_id': instance.remoteAccountId,
  'status': ?AccountDto._write_status(instance.status),
  'test_account': instance.testAccount,
  'updated_at': ?AccountDto._write_updatedAt(instance.updatedAt),
};
