// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageJobDto _$PageJobDtoFromJson(Map<String, dynamic> json) => PageJobDto(
  data: (json['data'] as List<dynamic>)
      .map((e) => Data3.fromJson(e as Map<String, dynamic>))
      .toList(),
  page: (json['page'] as num).toInt(),
  pageSize: (json['page_size'] as num).toInt(),
  total: (json['total'] as num).toInt(),
);

Map<String, dynamic> _$PageJobDtoToJson(PageJobDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
