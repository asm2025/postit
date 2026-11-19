// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_contact_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageContactListDto _$PageContactListDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageContactListDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => ContactListDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageContactListDto._read_page(json['page']),
    pageSize: PageContactListDto._read_pageSize(json['page_size']),
    total: PageContactListDto._read_total(json['total']),
    additionalProperties:
        PageContactListDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageContactListDto._readExtras(
            PageContactListDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageContactListDtoToJson(PageContactListDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageContactListDto._write_page(instance.page),
      'page_size': ?PageContactListDto._write_pageSize(instance.pageSize),
      'total': ?PageContactListDto._write_total(instance.total),
    };
