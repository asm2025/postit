// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'discovered_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DiscoveredAccountDto _$DiscoveredAccountDtoFromJson(
  Map<String, dynamic> json,
) => DiscoveredAccountDto(
  displayName: json['display_name'] as String,
  remoteId: json['remote_id'] as String,
  avatarUrl: json['avatar_url'] as String?,
  handle: json['handle'] as String?,
);

Map<String, dynamic> _$DiscoveredAccountDtoToJson(
  DiscoveredAccountDto instance,
) => <String, dynamic>{
  'avatar_url': instance.avatarUrl,
  'display_name': instance.displayName,
  'handle': instance.handle,
  'remote_id': instance.remoteId,
};
