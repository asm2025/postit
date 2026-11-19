// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_suppression_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageSuppressionDto _$PageSuppressionDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageSuppressionDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => SuppressionDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageSuppressionDto._read_page(json['page']),
    pageSize: PageSuppressionDto._read_pageSize(json['page_size']),
    total: PageSuppressionDto._read_total(json['total']),
    additionalProperties:
        PageSuppressionDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageSuppressionDto._readExtras(
            PageSuppressionDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageSuppressionDtoToJson(PageSuppressionDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageSuppressionDto._write_page(instance.page),
      'page_size': ?PageSuppressionDto._write_pageSize(instance.pageSize),
      'total': ?PageSuppressionDto._write_total(instance.total),
    };
