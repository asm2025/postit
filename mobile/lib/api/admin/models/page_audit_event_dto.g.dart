// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'page_audit_event_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PageAuditEventDto _$PageAuditEventDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['data', 'page', 'page_size', 'total'],
    disallowNullValues: const ['data', 'page', 'page_size', 'total'],
  );
  return PageAuditEventDto(
    data: (json['data'] as List<dynamic>)
        .map((e) => AuditEventDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    page: PageAuditEventDto._read_page(json['page']),
    pageSize: PageAuditEventDto._read_pageSize(json['page_size']),
    total: PageAuditEventDto._read_total(json['total']),
    additionalProperties:
        PageAuditEventDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PageAuditEventDto._readExtras(
            PageAuditEventDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PageAuditEventDtoToJson(PageAuditEventDto instance) =>
    <String, dynamic>{
      'data': instance.data.map((e) => e.toJson()).toList(),
      'page': ?PageAuditEventDto._write_page(instance.page),
      'page_size': ?PageAuditEventDto._write_pageSize(instance.pageSize),
      'total': ?PageAuditEventDto._write_total(instance.total),
    };
