// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_post_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PagePostSummaryDto _$PagePostSummaryDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PagePostSummaryDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => PostSummaryDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PagePostSummaryDto._read_page(json['page']),
    pageSize: PagePostSummaryDto._read_pageSize(json['page_size']),
    total: PagePostSummaryDto._read_total(json['total']),
    additionalProperties:
        PagePostSummaryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PagePostSummaryDto._readExtras(
            PagePostSummaryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PagePostSummaryDtoToJson(PagePostSummaryDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PagePostSummaryDto._write_page(instance.page),
      'page_size': ?PagePostSummaryDto._write_pageSize(instance.pageSize),
      'total': ?PagePostSummaryDto._write_total(instance.total),
    };
