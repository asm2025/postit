// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_publish_job_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PagePublishJobSummaryDto _$PagePublishJobSummaryDtoFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PagePublishJobSummaryDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => PublishJobSummaryDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PagePublishJobSummaryDto._read_page(json['page']),
    pageSize: PagePublishJobSummaryDto._read_pageSize(json['page_size']),
    total: PagePublishJobSummaryDto._read_total(json['total']),
    additionalProperties:
        PagePublishJobSummaryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PagePublishJobSummaryDto._readExtras(
            PagePublishJobSummaryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PagePublishJobSummaryDtoToJson(
  PagePublishJobSummaryDto instance,
) => <String, dynamic>{
  'data': instance.data.map((e) => e.toJson()).toList(),
  'page': ?PagePublishJobSummaryDto._write_page(instance.page),
  'page_size': ?PagePublishJobSummaryDto._write_pageSize(instance.pageSize),
  'total': ?PagePublishJobSummaryDto._write_total(instance.total),
};
