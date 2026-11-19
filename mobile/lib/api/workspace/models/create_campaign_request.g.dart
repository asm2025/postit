// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_campaign_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateCampaignRequest _$CreateCampaignRequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['color', 'ends_on', 'name', 'starts_on'],
    requiredKeys: const ['name'],
    disallowNullValues: const ['name'],
  );
  return CreateCampaignRequest(
    color: json['color'] as String?,
    endsOn: json['ends_on'] as String?,
    name: json['name'] as String,
    startsOn: json['starts_on'] as String?,
  );
}

Map<String, dynamic> _$CreateCampaignRequestToJson(
  CreateCampaignRequest instance,
) => <String, dynamic>{
  'color': ?instance.color,
  'ends_on': ?instance.endsOn,
  'name': instance.name,
  'starts_on': ?instance.startsOn,
};
