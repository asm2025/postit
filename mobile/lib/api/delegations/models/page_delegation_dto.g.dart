// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_delegation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageDelegationDto _$PageDelegationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageDelegationDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => DelegationDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageDelegationDto._read_page(json['page']),
    pageSize: PageDelegationDto._read_pageSize(json['page_size']),
    total: PageDelegationDto._read_total(json['total']),
    additionalProperties:
        PageDelegationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageDelegationDto._readExtras(
            PageDelegationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageDelegationDtoToJson(PageDelegationDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageDelegationDto._write_page(instance.page),
      'page_size': ?PageDelegationDto._write_pageSize(instance.pageSize),
      'total': ?PageDelegationDto._write_total(instance.total),
    };
