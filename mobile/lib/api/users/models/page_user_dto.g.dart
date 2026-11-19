// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageUserDto _$PageUserDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageUserDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => UserDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageUserDto._read_page(json['page']),
    pageSize: PageUserDto._read_pageSize(json['page_size']),
    total: PageUserDto._read_total(json['total']),
    additionalProperties:
        PageUserDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageUserDto._readExtras(
            PageUserDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageUserDtoToJson(PageUserDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageUserDto._write_page(instance.page),
      'page_size': ?PageUserDto._write_pageSize(instance.pageSize),
      'total': ?PageUserDto._write_total(instance.total),
    };
