// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportReportDto _$ContactImportReportDtoFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(json, disallowNullValues: const ['entries', 'totals']);
  return ContactImportReportDto(
    entries: (json['entries'] as List<dynamic>?)
        ?.map(
          (e) => ContactImportRejectionDto.fromJson(e as Map<String, dynamic>),
        )
        .toList(),
    totals: ContactImportReportDto._read_totals(json['totals']),
    additionalProperties:
        ContactImportReportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactImportReportDto._readExtras(
            ContactImportReportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactImportReportDtoToJson(
  ContactImportReportDto instance,
) => <String, dynamic>{
  'entries': ?instance.entries?.map((e) => e.toJson()).toList(),
  'totals': ?ContactImportReportDto._write_totals(instance.totals),
};
