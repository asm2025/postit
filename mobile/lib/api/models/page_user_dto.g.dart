// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageUserDto _$PageUserDtoFromJson(Map<String, dynamic> json) => PageUserDto(
  data: (json['data'] as List<dynamic>)
      .map((e) => Data6.fromJson(e as Map<String, dynamic>))
      .toList(),
  page: (json['page'] as num).toInt(),
  pageSize: (json['page_size'] as num).toInt(),
  total: (json['total'] as num).toInt(),
);

Map<String, dynamic> _$PageUserDtoToJson(PageUserDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
