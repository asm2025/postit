// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_target_group_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageTargetGroupDto _$PageTargetGroupDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageTargetGroupDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => TargetGroupDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageTargetGroupDto._read_page(json['page']),
    pageSize: PageTargetGroupDto._read_pageSize(json['page_size']),
    total: PageTargetGroupDto._read_total(json['total']),
    additionalProperties:
        PageTargetGroupDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageTargetGroupDto._readExtras(
            PageTargetGroupDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageTargetGroupDtoToJson(PageTargetGroupDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageTargetGroupDto._write_page(instance.page),
      'page_size': ?PageTargetGroupDto._write_pageSize(instance.pageSize),
      'total': ?PageTargetGroupDto._write_total(instance.total),
    };
