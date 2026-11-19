// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageJobDto _$PageJobDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageJobDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => JobDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageJobDto._read_page(json['page']),
    pageSize: PageJobDto._read_pageSize(json['page_size']),
    total: PageJobDto._read_total(json['total']),
    additionalProperties:
        PageJobDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageJobDto._readExtras(
            PageJobDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageJobDtoToJson(PageJobDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageJobDto._write_page(instance.page),
      'page_size': ?PageJobDto._write_pageSize(instance.pageSize),
      'total': ?PageJobDto._write_total(instance.total),
    };
