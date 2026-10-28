// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'grant_delegation_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GrantDelegationRequest _$GrantDelegationRequestFromJson(
  Map<String, dynamic> json,
) => GrantDelegationRequest(
  delegateId: json['delegate_id'] as String,
  level: LevelDto.fromJson(json['level'] as String),
  accountIds: (json['account_ids'] as List<dynamic>?)
      ?.map((e) => e as String)
      .toList(),
  expiresAt: json['expires_at'] == null
      ? null
      : DateTime.parse(json['expires_at'] as String),
);

Map<String, dynamic> _$GrantDelegationRequestToJson(
  GrantDelegationRequest instance,
) => <String, dynamic>{
  'account_ids': instance.accountIds,
  'delegate_id': instance.delegateId,
  'expires_at': instance.expiresAt?.toIso8601String(),
  'level': _$LevelDtoEnumMap[instance.level]!,
};

const _$LevelDtoEnumMap = {
  LevelDto.view: 'view',
  LevelDto.edit: 'edit',
  LevelDto.publish: 'publish',
  LevelDto.$unknown: r'$unknown',
};
