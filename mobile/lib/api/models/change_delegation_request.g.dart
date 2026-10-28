// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'change_delegation_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ChangeDelegationRequest _$ChangeDelegationRequestFromJson(
  Map<String, dynamic> json,
) => ChangeDelegationRequest(
  accountIds: (json['account_ids'] as List<dynamic>?)
      ?.map((e) => e as String)
      .toList(),
  allAccounts: json['all_accounts'] as bool?,
  expiresAt: json['expires_at'] == null
      ? null
      : DateTime.parse(json['expires_at'] as String),
  level: json['level'] == null
      ? null
      : LevelDto.fromJson(json['level'] as String),
);

Map<String, dynamic> _$ChangeDelegationRequestToJson(
  ChangeDelegationRequest instance,
) => <String, dynamic>{
  'account_ids': instance.accountIds,
  'all_accounts': instance.allAccounts,
  'expires_at': instance.expiresAt?.toIso8601String(),
  'level': _$LevelDtoEnumMap[instance.level],
};

const _$LevelDtoEnumMap = {
  LevelDto.view: 'view',
  LevelDto.edit: 'edit',
  LevelDto.publish: 'publish',
  LevelDto.$unknown: r'$unknown',
};
