// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_campaign_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageCampaignDto _$PageCampaignDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageCampaignDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => CampaignDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageCampaignDto._read_page(json['page']),
    pageSize: PageCampaignDto._read_pageSize(json['page_size']),
    total: PageCampaignDto._read_total(json['total']),
    additionalProperties:
        PageCampaignDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageCampaignDto._readExtras(
            PageCampaignDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageCampaignDtoToJson(PageCampaignDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageCampaignDto._write_page(instance.page),
      'page_size': ?PageCampaignDto._write_pageSize(instance.pageSize),
      'total': ?PageCampaignDto._write_total(instance.total),
    };
