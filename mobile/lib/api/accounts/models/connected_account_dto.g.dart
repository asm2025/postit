// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'connected_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConnectedAccountDto _$ConnectedAccountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account', 'change'],
    disallowNullValues: const ['account', 'change'],
  );
  return ConnectedAccountDto(
    account: AccountDto.fromJson(json['account'] as Map<String, dynamic>),
    change: ConnectedAccountDto._read_change(json['change']),
    additionalProperties:
        ConnectedAccountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ConnectedAccountDto._readExtras(
            ConnectedAccountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ConnectedAccountDtoToJson(
  ConnectedAccountDto instance,
) => <String, dynamic>{
  'account': instance.account.toJson(),
  'change': ?ConnectedAccountDto._write_change(instance.change),
};
