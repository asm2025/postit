// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_ai_generation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageAiGenerationDto _$PageAiGenerationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageAiGenerationDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => AiGenerationDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageAiGenerationDto._read_page(json['page']),
    pageSize: PageAiGenerationDto._read_pageSize(json['page_size']),
    total: PageAiGenerationDto._read_total(json['total']),
    additionalProperties:
        PageAiGenerationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageAiGenerationDto._readExtras(
            PageAiGenerationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageAiGenerationDtoToJson(
  PageAiGenerationDto instance,
) => <String, dynamic>{
  'data': instance.data.map((e) => e.toJson()).toList(),
  'page': ?PageAiGenerationDto._write_page(instance.page),
  'page_size': ?PageAiGenerationDto._write_pageSize(instance.pageSize),
  'total': ?PageAiGenerationDto._write_total(instance.total),
};
