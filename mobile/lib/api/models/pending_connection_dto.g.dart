// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'pending_connection_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PendingConnectionDto _$PendingConnectionDtoFromJson(
  Map<String, dynamic> json,
) => PendingConnectionDto(
  connect: ConnectKindDto.fromJson(json['connect'] as String),
  discovered: (json['discovered'] as List<dynamic>)
      .map((e) => DiscoveredAccountDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  expiresAt: DateTime.parse(json['expires_at'] as String),
  platform: json['platform'] as String,
);

Map<String, dynamic> _$PendingConnectionDtoToJson(
  PendingConnectionDto instance,
) => <String, dynamic>{
  'connect': _$ConnectKindDtoEnumMap[instance.connect]!,
  'discovered': instance.discovered,
  'expires_at': instance.expiresAt.toIso8601String(),
  'platform': instance.platform,
};

const _$ConnectKindDtoEnumMap = {
  ConnectKindDto.credentials: 'credentials',
  ConnectKindDto.oauth: 'oauth',
  ConnectKindDto.$unknown: r'$unknown',
};
