// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'campaign_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CampaignDto _$CampaignDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['archived', 'created_at', 'id', 'name', 'updated_at'],
    disallowNullValues: const [
      'archived',
      'created_at',
      'id',
      'name',
      'updated_at',
    ],
  );
  return CampaignDto(
    archived: json['archived'] as bool,
    color: json['color'] as String?,
    createdAt: CampaignDto._read_createdAt(json['created_at']),
    endsOn: json['ends_on'] as String?,
    id: json['id'] as String,
    name: json['name'] as String,
    startsOn: json['starts_on'] as String?,
    updatedAt: CampaignDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        CampaignDto._extras(json, 'additionalProperties') == null
        ? const {}
        : CampaignDto._readExtras(
            CampaignDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CampaignDtoToJson(CampaignDto instance) =>
    <String, dynamic>{
      'archived': instance.archived,
      'color': ?instance.color,
      'created_at': ?CampaignDto._write_createdAt(instance.createdAt),
      'ends_on': ?instance.endsOn,
      'id': instance.id,
      'name': instance.name,
      'starts_on': ?instance.startsOn,
      'updated_at': ?CampaignDto._write_updatedAt(instance.updatedAt),
    };
