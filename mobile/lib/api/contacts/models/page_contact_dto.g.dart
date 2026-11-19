// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_contact_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageContactDto _$PageContactDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageContactDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => ContactDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageContactDto._read_page(json['page']),
    pageSize: PageContactDto._read_pageSize(json['page_size']),
    total: PageContactDto._read_total(json['total']),
    additionalProperties:
        PageContactDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageContactDto._readExtras(
            PageContactDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageContactDtoToJson(PageContactDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageContactDto._write_page(instance.page),
      'page_size': ?PageContactDto._write_pageSize(instance.pageSize),
      'total': ?PageContactDto._write_total(instance.total),
    };
