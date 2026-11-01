// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_suppression_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageSuppressionDto _$PageSuppressionDtoFromJson(Map<String, dynamic> json) =>
    PageSuppressionDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => SuppressionDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageSuppressionDtoToJson(PageSuppressionDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
