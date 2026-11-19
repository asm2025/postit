// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'discovered_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DiscoveredAccountDto _$DiscoveredAccountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['display_name', 'remote_id'],
    disallowNullValues: const ['display_name', 'remote_id'],
  );
  return DiscoveredAccountDto(
    avatarUrl: json['avatar_url'] as String?,
    displayName: json['display_name'] as String,
    handle: json['handle'] as String?,
    remoteId: json['remote_id'] as String,
    additionalProperties:
        DiscoveredAccountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : DiscoveredAccountDto._readExtras(
            DiscoveredAccountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DiscoveredAccountDtoToJson(
  DiscoveredAccountDto instance,
) => <String, dynamic>{
  'avatar_url': ?instance.avatarUrl,
  'display_name': instance.displayName,
  'handle': ?instance.handle,
  'remote_id': instance.remoteId,
};
