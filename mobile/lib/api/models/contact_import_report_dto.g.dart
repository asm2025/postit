// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportReportDto _$ContactImportReportDtoFromJson(
  Map<String, dynamic> json,
) => ContactImportReportDto(
  entries: (json['entries'] as List<dynamic>?)
      ?.map(
        (e) => ContactImportRejectionDto.fromJson(e as Map<String, dynamic>),
      )
      .toList(),
  totals: (json['totals'] as Map<String, dynamic>?)?.map(
    (k, e) => MapEntry(k, (e as num).toInt()),
  ),
);

Map<String, dynamic> _$ContactImportReportDtoToJson(
  ContactImportReportDto instance,
) => <String, dynamic>{'entries': instance.entries, 'totals': instance.totals};
