// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'set_platform_enabled_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SetPlatformEnabledRequest _$SetPlatformEnabledRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['enabled'],
    requiredKeys: const ['enabled'],
    disallowNullValues: const ['enabled'],
  );
  return SetPlatformEnabledRequest(enabled: json['enabled'] as bool);
}

Map<String, dynamic> _$SetPlatformEnabledRequestToJson(
  SetPlatformEnabledRequest instance,
) => <String, dynamic>{'enabled': instance.enabled};
