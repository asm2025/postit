// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delegation_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DelegationNotice _$DelegationNoticeFromJson(Map<String, dynamic> json) =>
    DelegationNotice(
      delegationId: json['delegation_id'] as String,
      event: DelegationEvent.fromJson(json['event'] as String),
      partyName: json['party_name'] as String,
    );

Map<String, dynamic> _$DelegationNoticeToJson(DelegationNotice instance) =>
    <String, dynamic>{
      'delegation_id': instance.delegationId,
      'event': _$DelegationEventEnumMap[instance.event]!,
      'party_name': instance.partyName,
    };

const _$DelegationEventEnumMap = {
  DelegationEvent.granted: 'granted',
  DelegationEvent.changed: 'changed',
  DelegationEvent.revoked: 'revoked',
  DelegationEvent.left: 'left',
  DelegationEvent.expired: 'expired',
  DelegationEvent.limitEmptied: 'limit_emptied',
  DelegationEvent.$unknown: r'$unknown',
};
