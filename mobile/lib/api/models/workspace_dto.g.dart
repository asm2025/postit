// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'workspace_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

WorkspaceDto _$WorkspaceDtoFromJson(Map<String, dynamic> json) => WorkspaceDto(
  own: json['own'] as bool,
  owner: DelegationUserDto.fromJson(json['owner'] as Map<String, dynamic>),
  accountIds: (json['account_ids'] as List<dynamic>?)
      ?.map((e) => e as String)
      .toList(),
  allAccounts: json['all_accounts'] as bool?,
  delegationId: json['delegation_id'] as String?,
  level: json['level'] == null
      ? null
      : LevelDto.fromJson(json['level'] as String),
);

Map<String, dynamic> _$WorkspaceDtoToJson(WorkspaceDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'all_accounts': instance.allAccounts,
      'delegation_id': instance.delegationId,
      'level': _$LevelDtoEnumMap[instance.level],
      'own': instance.own,
      'owner': instance.owner,
    };

const _$LevelDtoEnumMap = {
  LevelDto.view: 'view',
  LevelDto.edit: 'edit',
  LevelDto.publish: 'publish',
  LevelDto.$unknown: r'$unknown',
};
