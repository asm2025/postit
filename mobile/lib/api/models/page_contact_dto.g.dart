// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_contact_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageContactDto _$PageContactDtoFromJson(Map<String, dynamic> json) =>
    PageContactDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => ContactDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageContactDtoToJson(PageContactDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
