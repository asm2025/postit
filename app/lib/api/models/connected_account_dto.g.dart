// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'connected_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConnectedAccountDto _$ConnectedAccountDtoFromJson(Map<String, dynamic> json) =>
    ConnectedAccountDto(
      account: AccountDto.fromJson(json['account'] as Map<String, dynamic>),
      change: AccountChangeDto.fromJson(json['change'] as String),
    );

Map<String, dynamic> _$ConnectedAccountDtoToJson(
  ConnectedAccountDto instance,
) => <String, dynamic>{
  'account': instance.account,
  'change': _$AccountChangeDtoEnumMap[instance.change]!,
};

const _$AccountChangeDtoEnumMap = {
  AccountChangeDto.created: 'created',
  AccountChangeDto.reactivated: 'reactivated',
  AccountChangeDto.moved: 'moved',
  AccountChangeDto.$unknown: r'$unknown',
};
