// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_campaign_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateCampaignRequest _$CreateCampaignRequestFromJson(
  Map<String, dynamic> json,
) => CreateCampaignRequest(
  name: json['name'] as String,
  color: json['color'] as String?,
  endsOn: json['ends_on'] == null
      ? null
      : DateTime.parse(json['ends_on'] as String),
  startsOn: json['starts_on'] == null
      ? null
      : DateTime.parse(json['starts_on'] as String),
);

Map<String, dynamic> _$CreateCampaignRequestToJson(
  CreateCampaignRequest instance,
) => <String, dynamic>{
  'color': instance.color,
  'ends_on': instance.endsOn?.toIso8601String(),
  'name': instance.name,
  'starts_on': instance.startsOn?.toIso8601String(),
};
