// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageMediaDto _$PageMediaDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageMediaDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => MediaDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageMediaDto._read_page(json['page']),
    pageSize: PageMediaDto._read_pageSize(json['page_size']),
    total: PageMediaDto._read_total(json['total']),
    additionalProperties:
        PageMediaDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageMediaDto._readExtras(
            PageMediaDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageMediaDtoToJson(PageMediaDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageMediaDto._write_page(instance.page),
      'page_size': ?PageMediaDto._write_pageSize(instance.pageSize),
      'total': ?PageMediaDto._write_total(instance.total),
    };
