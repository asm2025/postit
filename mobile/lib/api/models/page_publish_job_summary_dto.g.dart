// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_publish_job_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PagePublishJobSummaryDto _$PagePublishJobSummaryDtoFromJson(
  Map<String, dynamic> json,
) => PagePublishJobSummaryDto(
  data: (json['data'] as List<dynamic>)
      .map((e) => PublishJobSummaryDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  page: (json['page'] as num).toInt(),
  pageSize: (json['page_size'] as num).toInt(),
  total: (json['total'] as num).toInt(),
);

Map<String, dynamic> _$PagePublishJobSummaryDtoToJson(
  PagePublishJobSummaryDto instance,
) => <String, dynamic>{
  'data': instance.data,
  'page': instance.page,
  'page_size': instance.pageSize,
  'total': instance.total,
};
