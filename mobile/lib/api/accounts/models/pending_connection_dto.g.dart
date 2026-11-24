// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'pending_connection_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PendingConnectionDto _$PendingConnectionDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'connect',
      'discovered',
      'expires_at',
      'pending_id',
      'platform',
    ],
    disallowNullValues: const [
      'connect',
      'discovered',
      'expires_at',
      'pending_id',
      'platform',
    ],
  );
  return PendingConnectionDto(
    connect: PendingConnectionDto._read_connect(json['connect']),
    discovered: (json['discovered'] as List<dynamic>)
        .map((e) => DiscoveredAccountDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    expiresAt: PendingConnectionDto._read_expiresAt(json['expires_at']),
    pendingId: json['pending_id'] as String,
    platform: json['platform'] as String,
    additionalProperties:
        PendingConnectionDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PendingConnectionDto._readExtras(
            PendingConnectionDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PendingConnectionDtoToJson(
  PendingConnectionDto instance,
) => <String, dynamic>{
  'connect': ?PendingConnectionDto._write_connect(instance.connect),
  'discovered': instance.discovered.map((e) => e.toJson()).toList(),
  'expires_at': ?PendingConnectionDto._write_expiresAt(instance.expiresAt),
  'pending_id': instance.pendingId,
  'platform': instance.platform,
};
