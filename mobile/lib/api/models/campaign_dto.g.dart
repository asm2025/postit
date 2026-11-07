// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'campaign_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CampaignDto _$CampaignDtoFromJson(Map<String, dynamic> json) => CampaignDto(
  archived: json['archived'] as bool,
  createdAt: DateTime.parse(json['created_at'] as String),
  id: json['id'] as String,
  name: json['name'] as String,
  updatedAt: DateTime.parse(json['updated_at'] as String),
  color: json['color'] as String?,
  endsOn: json['ends_on'] == null
      ? null
      : DateTime.parse(json['ends_on'] as String),
  startsOn: json['starts_on'] == null
      ? null
      : DateTime.parse(json['starts_on'] as String),
);

Map<String, dynamic> _$CampaignDtoToJson(CampaignDto instance) =>
    <String, dynamic>{
      'archived': instance.archived,
      'color': instance.color,
      'created_at': instance.createdAt.toIso8601String(),
      'ends_on': instance.endsOn?.toIso8601String(),
      'id': instance.id,
      'name': instance.name,
      'starts_on': instance.startsOn?.toIso8601String(),
      'updated_at': instance.updatedAt.toIso8601String(),
    };
