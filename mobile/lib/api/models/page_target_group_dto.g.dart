// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_target_group_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageTargetGroupDto _$PageTargetGroupDtoFromJson(Map<String, dynamic> json) =>
    PageTargetGroupDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => TargetGroupDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageTargetGroupDtoToJson(PageTargetGroupDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
