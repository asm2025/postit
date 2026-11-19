// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delegation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DelegationDto _$DelegationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'account_ids',
      'all_accounts',
      'created_at',
      'delegate',
      'id',
      'level',
      'owner',
      'updated_at',
    ],
    disallowNullValues: const [
      'account_ids',
      'all_accounts',
      'created_at',
      'delegate',
      'id',
      'level',
      'owner',
      'updated_at',
    ],
  );
  return DelegationDto(
    accountIds: (json['account_ids'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    allAccounts: json['all_accounts'] as bool,
    createdAt: DelegationDto._read_createdAt(json['created_at']),
    createdBy: json['created_by'] as String?,
    delegate: DelegationUserDto.fromJson(
      json['delegate'] as Map<String, dynamic>,
    ),
    endedAt: DelegationDto._read_endedAt(json['ended_at']),
    endedReason: DelegationDto._read_endedReason(json['ended_reason']),
    expiresAt: DelegationDto._read_expiresAt(json['expires_at']),
    id: json['id'] as String,
    level: DelegationDto._read_level(json['level']),
    owner: DelegationUserDto.fromJson(json['owner'] as Map<String, dynamic>),
    updatedAt: DelegationDto._read_updatedAt(json['updated_at']),
    updatedBy: json['updated_by'] as String?,
    additionalProperties:
        DelegationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : DelegationDto._readExtras(
            DelegationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DelegationDtoToJson(DelegationDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'all_accounts': instance.allAccounts,
      'created_at': ?DelegationDto._write_createdAt(instance.createdAt),
      'created_by': ?instance.createdBy,
      'delegate': instance.delegate.toJson(),
      'ended_at': ?DelegationDto._write_endedAt(instance.endedAt),
      'ended_reason': ?DelegationDto._write_endedReason(instance.endedReason),
      'expires_at': ?DelegationDto._write_expiresAt(instance.expiresAt),
      'id': instance.id,
      'level': ?DelegationDto._write_level(instance.level),
      'owner': instance.owner.toJson(),
      'updated_at': ?DelegationDto._write_updatedAt(instance.updatedAt),
      'updated_by': ?instance.updatedBy,
    };
