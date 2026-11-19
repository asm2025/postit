// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'workspace_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

WorkspaceDto _$WorkspaceDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['own', 'owner'],
    disallowNullValues: const ['own', 'owner'],
  );
  return WorkspaceDto(
    accountIds: (json['account_ids'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    allAccounts: json['all_accounts'] as bool?,
    delegationId: json['delegation_id'] as String?,
    level: WorkspaceDto._read_level(json['level']),
    own: json['own'] as bool,
    owner: DelegationUserDto.fromJson(json['owner'] as Map<String, dynamic>),
    additionalProperties:
        WorkspaceDto._extras(json, 'additionalProperties') == null
        ? const {}
        : WorkspaceDto._readExtras(
            WorkspaceDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$WorkspaceDtoToJson(WorkspaceDto instance) =>
    <String, dynamic>{
      'account_ids': ?instance.accountIds,
      'all_accounts': ?instance.allAccounts,
      'delegation_id': ?instance.delegationId,
      'level': ?WorkspaceDto._write_level(instance.level),
      'own': instance.own,
      'owner': instance.owner.toJson(),
    };
