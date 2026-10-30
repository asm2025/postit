// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_audit_event_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageAuditEventDto _$PageAuditEventDtoFromJson(Map<String, dynamic> json) =>
    PageAuditEventDto(
      data: (json['data'] as List<dynamic>)
          .map((e) => AuditEventDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      page: (json['page'] as num).toInt(),
      pageSize: (json['page_size'] as num).toInt(),
      total: (json['total'] as num).toInt(),
    );

Map<String, dynamic> _$PageAuditEventDtoToJson(PageAuditEventDto instance) =>
    <String, dynamic>{
      'data': instance.data,
      'page': instance.page,
      'page_size': instance.pageSize,
      'total': instance.total,
    };
