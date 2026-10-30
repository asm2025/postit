// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_notification_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageNotificationDto _$PageNotificationDtoFromJson(Map<String, dynamic> json) =>
    PageNotificationDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => NotificationDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageNotificationDtoToJson(
  PageNotificationDto instance,
) => <String, dynamic>{
  'data': instance.data,
  'page': instance.page,
  'page_size': instance.pageSize,
  'total': instance.total,
};
