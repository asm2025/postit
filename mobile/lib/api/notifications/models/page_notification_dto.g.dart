// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_notification_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageNotificationDto _$PageNotificationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageNotificationDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => NotificationDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageNotificationDto._read_page(json['page']),
    pageSize: PageNotificationDto._read_pageSize(json['page_size']),
    total: PageNotificationDto._read_total(json['total']),
    additionalProperties:
        PageNotificationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageNotificationDto._readExtras(
            PageNotificationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageNotificationDtoToJson(
  PageNotificationDto instance,
) => <String, dynamic>{
  'data': instance.data.map((e) => e.toJson()).toList(),
  'page': ?PageNotificationDto._write_page(instance.page),
  'page_size': ?PageNotificationDto._write_pageSize(instance.pageSize),
  'total': ?PageNotificationDto._write_total(instance.total),
};
