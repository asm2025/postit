// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_campaign_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchCampaignRequest _$PatchCampaignRequestFromJson(
  Map<String, dynamic> json,
) => PatchCampaignRequest(
  archived: json['archived'] as bool?,
  color: json['color'] as String?,
  endsOn: json['ends_on'] == null
      ? null
      : DateTime.parse(json['ends_on'] as String),
  name: json['name'] as String?,
  startsOn: json['starts_on'] == null
      ? null
      : DateTime.parse(json['starts_on'] as String),
);

Map<String, dynamic> _$PatchCampaignRequestToJson(
  PatchCampaignRequest instance,
) => <String, dynamic>{
  'archived': instance.archived,
  'color': instance.color,
  'ends_on': instance.endsOn?.toIso8601String(),
  'name': instance.name,
  'starts_on': instance.startsOn?.toIso8601String(),
};
