// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_media_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageMediaDto _$PageMediaDtoFromJson(Map<String, dynamic> json) => PageMediaDto(
  data: (json['data'] as List<dynamic>)
      .map((e) => MediaDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  page: (json['page'] as num).toInt(),
  pageSize: (json['page_size'] as num).toInt(),
  total: (json['total'] as num).toInt(),
);

Map<String, dynamic> _$PageMediaDtoToJson(PageMediaDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
