// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageAccountDto _$PageAccountDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageAccountDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => AccountDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageAccountDto._read_page(json['page']),
    pageSize: PageAccountDto._read_pageSize(json['page_size']),
    total: PageAccountDto._read_total(json['total']),
    additionalProperties:
        PageAccountDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageAccountDto._readExtras(
            PageAccountDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageAccountDtoToJson(PageAccountDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageAccountDto._write_page(instance.page),
      'page_size': ?PageAccountDto._write_pageSize(instance.pageSize),
      'total': ?PageAccountDto._write_total(instance.total),
    };
