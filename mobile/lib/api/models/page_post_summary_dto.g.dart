// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_post_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PagePostSummaryDto _$PagePostSummaryDtoFromJson(Map<String, dynamic> json) =>
    PagePostSummaryDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => PostSummaryDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PagePostSummaryDtoToJson(PagePostSummaryDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
