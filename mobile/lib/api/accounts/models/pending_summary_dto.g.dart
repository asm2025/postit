// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'pending_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PendingSummaryDto _$PendingSummaryDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'connect',
      'created_at',
      'expires_at',
      'pending_id',
      'platform',
    ],
    disallowNullValues: const [
      'connect',
      'created_at',
      'expires_at',
      'pending_id',
      'platform',
    ],
  );
  return PendingSummaryDto(
    connect: PendingSummaryDto._read_connect(json['connect']),
    createdAt: PendingSummaryDto._read_createdAt(json['created_at']),
    expiresAt: PendingSummaryDto._read_expiresAt(json['expires_at']),
    pendingId: json['pending_id'] as String,
    platform: json['platform'] as String,
    additionalProperties:
        PendingSummaryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PendingSummaryDto._readExtras(
            PendingSummaryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PendingSummaryDtoToJson(PendingSummaryDto instance) =>
    <String, dynamic>{
      'connect': ?PendingSummaryDto._write_connect(instance.connect),
      'created_at': ?PendingSummaryDto._write_createdAt(instance.createdAt),
      'expires_at': ?PendingSummaryDto._write_expiresAt(instance.expiresAt),
      'pending_id': instance.pendingId,
      'platform': instance.platform,
    };
