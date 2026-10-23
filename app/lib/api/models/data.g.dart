// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'data.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Data _$DataFromJson(Map<String, dynamic> json) => Data(
  createdAt: DateTime.parse(json['created_at'] as String),
  displayName: json['display_name'] as String,
  id: json['id'] as String,
  platform: json['platform'] as String,
  remoteAccountId: json['remote_account_id'] as String,
  status: AccountStatusDto.fromJson(json['status'] as String),
  testAccount: json['test_account'] as bool,
  updatedAt: DateTime.parse(json['updated_at'] as String),
  avatarUrl: json['avatar_url'] as String?,
  connect: json['connect'] == null
      ? null
      : ConnectKindDto.fromJson(json['connect'] as String),
  lastCheckedAt: json['last_checked_at'] == null
      ? null
      : DateTime.parse(json['last_checked_at'] as String),
);

Map<String, dynamic> _$DataToJson(Data instance) => <String, dynamic>{
  'avatar_url': instance.avatarUrl,
  'connect': _$ConnectKindDtoEnumMap[instance.connect],
  'created_at': instance.createdAt.toIso8601String(),
  'display_name': instance.displayName,
  'id': instance.id,
  'last_checked_at': instance.lastCheckedAt?.toIso8601String(),
  'platform': instance.platform,
  'remote_account_id': instance.remoteAccountId,
  'status': _$AccountStatusDtoEnumMap[instance.status]!,
  'test_account': instance.testAccount,
  'updated_at': instance.updatedAt.toIso8601String(),
};

const _$ConnectKindDtoEnumMap = {
  ConnectKindDto.credentials: 'credentials',
  ConnectKindDto.oauth: 'oauth',
  ConnectKindDto.$unknown: r'$unknown',
};

const _$AccountStatusDtoEnumMap = {
  AccountStatusDto.active: 'active',
  AccountStatusDto.needsReconnect: 'needs_reconnect',
  AccountStatusDto.revoked: 'revoked',
  AccountStatusDto.disconnected: 'disconnected',
  AccountStatusDto.$unknown: r'$unknown',
};
