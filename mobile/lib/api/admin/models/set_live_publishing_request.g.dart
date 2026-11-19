// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'set_live_publishing_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SetLivePublishingRequest _$SetLivePublishingRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['live'],
    requiredKeys: const ['live'],
    disallowNullValues: const ['live'],
  );
  return SetLivePublishingRequest(live: json['live'] as bool);
}

Map<String, dynamic> _$SetLivePublishingRequestToJson(
  SetLivePublishingRequest instance,
) => <String, dynamic>{'live': instance.live};
