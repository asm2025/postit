// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_contact_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageContactListDto _$PageContactListDtoFromJson(Map<String, dynamic> json) =>
    PageContactListDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => ContactListDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageContactListDtoToJson(PageContactListDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
