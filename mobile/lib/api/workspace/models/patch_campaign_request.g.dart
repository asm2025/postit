// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_campaign_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchCampaignRequest _$PatchCampaignRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['archived', 'color', 'ends_on', 'name', 'starts_on'],
  );
  return PatchCampaignRequest(
    archived: json['archived'] as bool?,
    color: json['color'] as String?,
    endsOn: json['ends_on'] as String?,
    name: json['name'] as String?,
    startsOn: json['starts_on'] as String?,
  );
}

Map<String, dynamic> _$PatchCampaignRequestToJson(
  PatchCampaignRequest instance,
) => <String, dynamic>{
  'archived': ?instance.archived,
  'color': ?instance.color,
  'ends_on': ?instance.endsOn,
  'name': ?instance.name,
  'starts_on': ?instance.startsOn,
};
