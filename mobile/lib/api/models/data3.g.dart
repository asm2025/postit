// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'data3.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Data3 _$Data3FromJson(Map<String, dynamic> json) => Data3(
  accountIds: (json['account_ids'] as List<dynamic>)
      .map((e) => e as String)
      .toList(),
  allAccounts: json['all_accounts'] as bool,
  createdAt: DateTime.parse(json['created_at'] as String),
  delegate: DelegationUserDto.fromJson(
    json['delegate'] as Map<String, dynamic>,
  ),
  id: json['id'] as String,
  level: LevelDto.fromJson(json['level'] as String),
  owner: DelegationUserDto.fromJson(json['owner'] as Map<String, dynamic>),
  updatedAt: DateTime.parse(json['updated_at'] as String),
  createdBy: json['created_by'] as String?,
  endedAt: json['ended_at'] == null
      ? null
      : DateTime.parse(json['ended_at'] as String),
  endedReason: json['ended_reason'] == null
      ? null
      : EndedReasonDto.fromJson(json['ended_reason'] as String),
  expiresAt: json['expires_at'] == null
      ? null
      : DateTime.parse(json['expires_at'] as String),
  updatedBy: json['updated_by'] as String?,
);

Map<String, dynamic> _$Data3ToJson(Data3 instance) => <String, dynamic>{
  'account_ids': instance.accountIds,
  'all_accounts': instance.allAccounts,
  'created_at': instance.createdAt.toIso8601String(),
  'created_by': instance.createdBy,
  'delegate': instance.delegate,
  'ended_at': instance.endedAt?.toIso8601String(),
  'ended_reason': _$EndedReasonDtoEnumMap[instance.endedReason],
  'expires_at': instance.expiresAt?.toIso8601String(),
  'id': instance.id,
  'level': _$LevelDtoEnumMap[instance.level]!,
  'owner': instance.owner,
  'updated_at': instance.updatedAt.toIso8601String(),
  'updated_by': instance.updatedBy,
};

const _$EndedReasonDtoEnumMap = {
  EndedReasonDto.revoked: 'revoked',
  EndedReasonDto.left: 'left',
  EndedReasonDto.expired: 'expired',
  EndedReasonDto.$unknown: r'$unknown',
};

const _$LevelDtoEnumMap = {
  LevelDto.view: 'view',
  LevelDto.edit: 'edit',
  LevelDto.publish: 'publish',
  LevelDto.$unknown: r'$unknown',
};
