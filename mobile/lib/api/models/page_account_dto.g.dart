// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_account_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageAccountDto _$PageAccountDtoFromJson(Map<String, dynamic> json) =>
    PageAccountDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => AccountDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageAccountDtoToJson(PageAccountDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
