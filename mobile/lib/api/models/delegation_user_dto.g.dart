// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delegation_user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DelegationUserDto _$DelegationUserDtoFromJson(Map<String, dynamic> json) =>
    DelegationUserDto(
      displayName: json['display_name'] as String,
      id: json['id'] as String,
      status: StatusDto.fromJson(json['status'] as String),
    );

Map<String, dynamic> _$DelegationUserDtoToJson(DelegationUserDto instance) =>
    <String, dynamic>{
      'display_name': instance.displayName,
      'id': instance.id,
      'status': _$StatusDtoEnumMap[instance.status]!,
    };

const _$StatusDtoEnumMap = {
  StatusDto.pending: 'pending',
  StatusDto.active: 'active',
  StatusDto.disabled: 'disabled',
  StatusDto.deleting: 'deleting',
  StatusDto.$unknown: r'$unknown',
};
